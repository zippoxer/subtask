package ops

import (
	"context"
	"fmt"
	"strings"

	"github.com/zippoxer/subtask/pkg/git"
	"github.com/zippoxer/subtask/pkg/task"
	"github.com/zippoxer/subtask/pkg/task/index"
	"github.com/zippoxer/subtask/pkg/task/queue"
)

// QueueMergeOptions configures the merge queue operation.
type QueueMergeOptions struct {
	Strategy    queue.Strategy // Ordering strategy
	MinStage    string         // Required minimum stage (default: "ready")
	DryRun      bool           // Show plan without merging
	StopOnError bool           // Stop on first failure
	NoRebase    bool           // Skip auto-rebase on conflicts
	RepoDir     string         // Repository directory
}

// QueueMergeResult contains the results of a queue merge operation.
type QueueMergeResult struct {
	Merged  []string          // Successfully merged tasks
	Rebased []string          // Tasks that were auto-rebased
	Failed  map[string]string // Task name -> error message
	Skipped []string          // Tasks skipped (blocked or dry-run)
}

// MergeQueue merges all ready tasks in queue order.
func MergeQueue(ctx context.Context, idx *index.Index, opts QueueMergeOptions, logger Logger) (QueueMergeResult, error) {
	if opts.Strategy == "" {
		opts.Strategy = queue.StrategyConflictAware
	}
	if opts.MinStage == "" {
		opts.MinStage = "ready"
	}
	if opts.RepoDir == "" {
		opts.RepoDir = "."
	}

	result := QueueMergeResult{
		Failed: make(map[string]string),
	}

	// Build the queue
	items, err := queue.Build(ctx, idx, queue.Options{
		Strategy:   opts.Strategy,
		MinStage:   opts.MinStage,
		IncludeAll: false, // Only mergeable tasks
		RepoDir:    opts.RepoDir,
	})
	if err != nil {
		return result, fmt.Errorf("failed to build queue: %w", err)
	}

	if len(items) == 0 {
		return result, nil
	}

	logInfo(logger, fmt.Sprintf("Merge queue: %d tasks ready", len(items)))

	for i, item := range items {
		taskName := item.Name

		// In dry-run mode, just report what would happen
		if opts.DryRun {
			logInfo(logger, fmt.Sprintf("[%d/%d] Would merge: %s (%s)",
				i+1, len(items), taskName, queue.ShortPriorityLabel(item.Priority)))
			result.Skipped = append(result.Skipped, taskName)
			continue
		}

		logInfo(logger, fmt.Sprintf("[%d/%d] Merging: %s", i+1, len(items), taskName))

		// Re-check for conflicts (base may have moved after previous merges)
		if !opts.NoRebase && item.Workspace != "" {
			conflicts, err := git.MergeConflictFiles(item.Workspace, item.BaseBranch, "HEAD")
			if err == nil && len(conflicts) > 0 {
				// Try auto-rebase
				logInfo(logger, fmt.Sprintf("  Rebasing onto %s...", item.BaseBranch))
				if err := AutoRebase(taskName, item.Workspace, item.BaseBranch, logger); err != nil {
					result.Failed[taskName] = fmt.Sprintf("rebase failed: %v", err)
					if opts.StopOnError {
						return result, fmt.Errorf("merge queue stopped: rebase failed for %s: %w", taskName, err)
					}
					continue
				}
				result.Rebased = append(result.Rebased, taskName)
			}
		}

		// Attempt merge
		commitMsg := fmt.Sprintf("Merge %s", item.Title)
		if item.Title == "" {
			commitMsg = fmt.Sprintf("Merge task %s", taskName)
		}

		mergeResult, err := MergeTask(taskName, commitMsg, logger)
		if err != nil {
			result.Failed[taskName] = err.Error()
			if opts.StopOnError {
				return result, fmt.Errorf("merge queue stopped: %s failed: %w", taskName, err)
			}
			logWarning(logger, fmt.Sprintf("  Failed: %v", err))
			continue
		}

		if mergeResult.AlreadyMerged || mergeResult.AlreadyClosed {
			result.Skipped = append(result.Skipped, taskName)
			continue
		}

		result.Merged = append(result.Merged, taskName)
	}

	return result, nil
}

// AutoRebase rebases a task's branch onto the current base branch.
// This is used to resolve conflicts that arise when the base branch moves.
func AutoRebase(taskName, workspace, baseBranch string, logger Logger) error {
	var rebaseErr error
	locked, err := task.TryWithLock(taskName, func() error {
		rebaseErr = autoRebaseUnlocked(workspace, baseBranch, logger)
		return rebaseErr
	})
	if err != nil {
		return err
	}
	if !locked {
		return fmt.Errorf("task %q is busy (another operation is in progress)", taskName)
	}
	return rebaseErr
}

func autoRebaseUnlocked(workspace, baseBranch string, logger Logger) error {
	// Get current branch
	currentBranch, err := git.CurrentBranch(workspace)
	if err != nil {
		return fmt.Errorf("failed to get current branch: %w", err)
	}

	if currentBranch == "HEAD" {
		return fmt.Errorf("workspace is in detached HEAD state")
	}

	// Ensure workspace is clean
	clean, err := git.IsClean(workspace)
	if err != nil {
		return fmt.Errorf("failed to check workspace status: %w", err)
	}
	if !clean {
		return fmt.Errorf("workspace has uncommitted changes")
	}

	// Attempt rebase
	if err := git.RebaseOnto(workspace, baseBranch); err != nil {
		// Extract conflict info from error
		errStr := err.Error()
		if strings.Contains(errStr, "CONFLICT") {
			return fmt.Errorf("conflicts detected:\n%s", errStr)
		}
		return fmt.Errorf("rebase failed: %w", err)
	}

	logSuccess(logger, fmt.Sprintf("  Rebased onto %s", baseBranch))
	return nil
}

// CheckQueueHealth returns information about the merge queue state.
func CheckQueueHealth(ctx context.Context, idx *index.Index, opts queue.Options) (QueueHealthInfo, error) {
	items, err := queue.Build(ctx, idx, queue.Options{
		Strategy:   opts.Strategy,
		MinStage:   opts.MinStage,
		IncludeAll: true, // Include blocked to count them
		RepoDir:    opts.RepoDir,
	})
	if err != nil {
		return QueueHealthInfo{}, err
	}

	info := QueueHealthInfo{}
	for _, item := range items {
		info.Total++
		if item.CanMerge {
			info.Ready++
		} else {
			info.Blocked++
		}
		if len(item.ConflictFiles) > 0 {
			info.WithConflicts++
		}
		if len(item.OverlapsWith) > 0 {
			info.WithOverlaps++
		}
	}

	return info, nil
}

// QueueHealthInfo provides summary statistics about the merge queue.
type QueueHealthInfo struct {
	Total         int // Total tasks in queue
	Ready         int // Tasks ready to merge
	Blocked       int // Tasks blocked from merging
	WithConflicts int // Tasks with base branch conflicts
	WithOverlaps  int // Tasks with file overlaps with other tasks
}
