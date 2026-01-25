package main

import (
	"context"
	"fmt"
	"strings"

	"github.com/zippoxer/subtask/pkg/render"
	"github.com/zippoxer/subtask/pkg/task/index"
	"github.com/zippoxer/subtask/pkg/task/ops"
	"github.com/zippoxer/subtask/pkg/task/queue"
)

// QueueCmd implements 'subtask queue' with subcommands.
type QueueCmd struct {
	List  QueueListCmd  `cmd:"" help:"Show merge queue with ordering"`
	Merge QueueMergeCmd `cmd:"" help:"Merge ready tasks in queue order"`
}

// QueueListCmd implements 'subtask queue list'.
type QueueListCmd struct {
	Strategy string `default:"conflict-aware" enum:"fifo,priority,conflict-aware" help:"Ordering strategy: fifo, priority, conflict-aware"`
	Stage    string `help:"Filter by minimum stage (e.g., ready)"`
	All      bool   `short:"a" help:"Include blocked tasks"`
}

// Run executes the queue list command.
func (c *QueueListCmd) Run() error {
	idx, err := index.OpenDefault()
	if err != nil {
		return fmt.Errorf("failed to open index: %w", err)
	}
	defer idx.Close()

	ctx := context.Background()

	// Refresh to get latest state
	if err := idx.Refresh(ctx, index.RefreshPolicy{
		Git: index.GitPolicy{
			Mode:               index.GitOpenOnly,
			IncludeIntegration: true,
			IncludeConflicts:   true,
		},
	}); err != nil {
		printWarning(fmt.Sprintf("failed to refresh index: %v", err))
	}

	// Build queue
	items, err := queue.Build(ctx, idx, queue.Options{
		Strategy:   queue.Strategy(c.Strategy),
		MinStage:   c.Stage,
		IncludeAll: c.All,
	})
	if err != nil {
		return fmt.Errorf("failed to build queue: %w", err)
	}

	if len(items) == 0 {
		fmt.Println("Merge queue is empty.")
		if !c.All {
			fmt.Println("\nUse --all to include blocked tasks.")
		}
		return nil
	}

	// Print queue
	fmt.Printf("Merge Queue (%d tasks, strategy: %s)\n\n", len(items), c.Strategy)

	// Table header
	if render.Pretty {
		fmt.Printf("  %-4s  %-30s  %-8s  %-10s  %s\n",
			"#", "TASK", "PRIORITY", "STAGE", "STATUS")
		fmt.Printf("  %-4s  %-30s  %-8s  %-10s  %s\n",
			"──", "────────────────────────────────", "────────", "──────────", "──────────────────────────")
	} else {
		fmt.Printf("  %-4s  %-30s  %-8s  %-10s  %s\n",
			"#", "TASK", "PRIORITY", "STAGE", "STATUS")
	}

	for i, item := range items {
		name := item.Name
		if len(name) > 30 {
			name = name[:27] + "..."
		}

		stage := item.Stage
		if stage == "" {
			stage = "-"
		}

		status := "ready"
		if !item.CanMerge {
			status = item.BlockedReason
			if len(status) > 30 {
				status = status[:27] + "..."
			}
		}

		// Add conflict/overlap indicators
		if len(item.ConflictFiles) > 0 {
			status += fmt.Sprintf(" [%d conflicts]", len(item.ConflictFiles))
		}
		if len(item.OverlapsWith) > 0 && item.CanMerge {
			status += fmt.Sprintf(" [overlaps: %d]", len(item.OverlapsWith))
		}

		fmt.Printf("  %-4d  %-30s  %-8s  %-10s  %s\n",
			i+1, name, queue.ShortPriorityLabel(item.Priority), stage, status)
	}

	// Summary
	ready := 0
	blocked := 0
	for _, item := range items {
		if item.CanMerge {
			ready++
		} else {
			blocked++
		}
	}

	fmt.Println()
	if ready > 0 {
		fmt.Printf("%d task(s) ready to merge.\n", ready)
		fmt.Printf("\nRun: subtask queue merge\n")
	}
	if blocked > 0 && c.All {
		fmt.Printf("%d task(s) blocked.\n", blocked)
	}

	return nil
}

// QueueMergeCmd implements 'subtask queue merge'.
type QueueMergeCmd struct {
	Strategy    string `default:"conflict-aware" enum:"fifo,priority,conflict-aware" help:"Ordering strategy"`
	Stage       string `default:"ready" help:"Required minimum stage"`
	DryRun      bool   `help:"Show plan without merging"`
	StopOnError bool   `help:"Stop on first failure"`
	NoRebase    bool   `help:"Skip auto-rebase on conflicts"`
}

// Run executes the queue merge command.
func (c *QueueMergeCmd) Run() error {
	idx, err := index.OpenDefault()
	if err != nil {
		return fmt.Errorf("failed to open index: %w", err)
	}
	defer idx.Close()

	ctx := context.Background()

	// Refresh to get latest state
	if err := idx.Refresh(ctx, index.RefreshPolicy{
		Git: index.GitPolicy{
			Mode:               index.GitOpenOnly,
			IncludeIntegration: true,
			IncludeConflicts:   true,
		},
	}); err != nil {
		printWarning(fmt.Sprintf("failed to refresh index: %v", err))
	}

	result, err := ops.MergeQueue(ctx, idx, ops.QueueMergeOptions{
		Strategy:    queue.Strategy(c.Strategy),
		MinStage:    c.Stage,
		DryRun:      c.DryRun,
		StopOnError: c.StopOnError,
		NoRebase:    c.NoRebase,
	}, cliOpsLogger{})

	// Print summary
	fmt.Println()
	if c.DryRun {
		fmt.Println("Dry run completed.")
	}

	if len(result.Merged) > 0 {
		printSuccess(fmt.Sprintf("Merged %d task(s): %s", len(result.Merged), strings.Join(result.Merged, ", ")))
	}

	if len(result.Rebased) > 0 {
		printInfo(fmt.Sprintf("Rebased %d task(s): %s", len(result.Rebased), strings.Join(result.Rebased, ", ")))
	}

	if len(result.Failed) > 0 {
		fmt.Println()
		printWarning(fmt.Sprintf("Failed %d task(s):", len(result.Failed)))
		for name, reason := range result.Failed {
			fmt.Printf("  %s: %s\n", name, reason)
		}
	}

	if len(result.Skipped) > 0 && !c.DryRun {
		fmt.Printf("Skipped %d task(s): %s\n", len(result.Skipped), strings.Join(result.Skipped, ", "))
	}

	// Refresh integration cache after merges
	if len(result.Merged) > 0 {
		if err := idx.Refresh(ctx, index.RefreshPolicy{
			Git: index.GitPolicy{
				Mode:               index.GitTasks,
				Tasks:              result.Merged,
				IncludeIntegration: true,
			},
		}); err != nil {
			printWarning(fmt.Sprintf("failed to refresh integration cache: %v", err))
		}
	}

	return err
}
