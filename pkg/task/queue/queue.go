// Package queue provides merge queue ordering and conflict detection.
package queue

import (
	"context"
	"encoding/json"
	"sort"

	"github.com/zippoxer/subtask/pkg/git"
	"github.com/zippoxer/subtask/pkg/task"
	"github.com/zippoxer/subtask/pkg/task/index"
)

// Strategy defines how the merge queue is ordered.
type Strategy string

const (
	// StrategyFIFO orders tasks by creation time (oldest first).
	StrategyFIFO Strategy = "fifo"
	// StrategyPriority orders by priority then age.
	StrategyPriority Strategy = "priority"
	// StrategyConflictAware orders to minimize cascading conflicts (default).
	StrategyConflictAware Strategy = "conflict-aware"
)

// QueueItem represents a task in the merge queue.
type QueueItem struct {
	Name         string
	Title        string
	Priority     int // 1-5 (P1=critical, P5=low)
	Stage        string
	TaskStatus   task.TaskStatus
	WorkerStatus task.WorkerStatus
	Workspace    string
	BaseBranch   string

	// Computed fields
	Score         float64  // Computed merge priority score
	ConflictFiles []string // Files that would conflict with base branch
	OverlapsWith  []string // Other tasks with shared changed files
	CanMerge      bool     // Whether task can be merged now
	BlockedReason string   // Why task cannot be merged (if blocked)

	// Internal
	changedFiles []string // Files changed by this task
	lastHistory  int64    // For age calculation
}

// Options configures queue building.
type Options struct {
	Strategy     Strategy // Ordering strategy (default: conflict-aware)
	MinStage     string   // Minimum stage required (e.g., "ready")
	IncludeAll   bool     // Include blocked tasks
	RepoDir      string   // Repository directory for git operations
}

// Build creates an ordered merge queue from indexed tasks.
func Build(ctx context.Context, idx *index.Index, opts Options) ([]QueueItem, error) {
	if opts.Strategy == "" {
		opts.Strategy = StrategyConflictAware
	}
	if opts.RepoDir == "" {
		opts.RepoDir = "."
	}

	// Get all open tasks
	items, err := idx.ListOpen(ctx)
	if err != nil {
		return nil, err
	}

	// Convert to queue items
	queue := make([]QueueItem, 0, len(items))
	for _, item := range items {
		qi := QueueItem{
			Name:         item.Name,
			Title:        item.Title,
			Priority:     item.Priority,
			Stage:        item.Stage,
			TaskStatus:   item.TaskStatus,
			WorkerStatus: item.WorkerStatus,
			Workspace:    item.Workspace,
			BaseBranch:   item.BaseBranch,
			lastHistory:  item.LastHistory.UnixNano(),
		}

		// Skip merged/closed tasks
		if item.TaskStatus != task.TaskStatusOpen {
			continue
		}

		// Check maturity (can merge now?)
		qi.CanMerge, qi.BlockedReason = CheckMaturity(&qi, opts.MinStage)

		// Get conflict info from index if available
		rec, ok, err := idx.Get(ctx, item.Name)
		if err == nil && ok && rec.ConflictFilesJSON != "" {
			var files []string
			if json.Unmarshal([]byte(rec.ConflictFilesJSON), &files) == nil {
				qi.ConflictFiles = files
			}
		}

		// Get changed files for overlap detection (if workspace exists)
		if qi.Workspace != "" {
			if base, err := git.ResolveDiffBase(qi.Workspace, "HEAD", qi.BaseBranch); err == nil {
				if files, err := git.DiffNameStatus(qi.Workspace, base); err == nil {
					qi.changedFiles = make([]string, 0, len(files))
					for f := range files {
						qi.changedFiles = append(qi.changedFiles, f)
					}
				}
			}
		}

		if !opts.IncludeAll && !qi.CanMerge {
			continue
		}

		queue = append(queue, qi)
	}

	// Build overlap matrix and compute overlaps
	if opts.Strategy == StrategyConflictAware {
		matrix := BuildOverlapMatrix(queue)
		for i := range queue {
			queue[i].OverlapsWith = matrix[queue[i].Name]
		}
	}

	// Calculate scores
	var maxAge int64
	for _, qi := range queue {
		if qi.lastHistory > maxAge {
			maxAge = qi.lastHistory
		}
	}
	for i := range queue {
		queue[i].Score = CalculateScore(&queue[i], maxAge)
	}

	// Sort by strategy
	switch opts.Strategy {
	case StrategyFIFO:
		sort.Slice(queue, func(i, j int) bool {
			return queue[i].lastHistory < queue[j].lastHistory
		})
	case StrategyPriority:
		sort.Slice(queue, func(i, j int) bool {
			if queue[i].Priority != queue[j].Priority {
				return queue[i].Priority < queue[j].Priority // P1 < P5
			}
			return queue[i].lastHistory < queue[j].lastHistory
		})
	case StrategyConflictAware:
		queue = OptimalMergeOrder(queue)
	}

	return queue, nil
}

// PriorityLabel returns a human-readable label for priority.
func PriorityLabel(p int) string {
	switch p {
	case 1:
		return "P1 (critical)"
	case 2:
		return "P2 (high)"
	case 3:
		return "P3 (normal)"
	case 4:
		return "P4 (low)"
	case 5:
		return "P5 (lowest)"
	default:
		return "P3 (normal)"
	}
}

// ShortPriorityLabel returns a short label for priority.
func ShortPriorityLabel(p int) string {
	switch p {
	case 1:
		return "P1"
	case 2:
		return "P2"
	case 3:
		return "P3"
	case 4:
		return "P4"
	case 5:
		return "P5"
	default:
		return "P3"
	}
}
