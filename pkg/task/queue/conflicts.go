package queue

import (
	"sort"
)

// BuildOverlapMatrix builds a map of task name -> tasks that share changed files.
// This helps identify which tasks might conflict with each other.
func BuildOverlapMatrix(items []QueueItem) map[string][]string {
	// Build file -> tasks map
	fileToTasks := make(map[string][]string)
	for _, item := range items {
		for _, f := range item.changedFiles {
			fileToTasks[f] = append(fileToTasks[f], item.Name)
		}
	}

	// Build task -> overlapping tasks map
	overlaps := make(map[string][]string)
	for _, item := range items {
		seen := make(map[string]bool)
		for _, f := range item.changedFiles {
			for _, other := range fileToTasks[f] {
				if other != item.Name && !seen[other] {
					seen[other] = true
					overlaps[item.Name] = append(overlaps[item.Name], other)
				}
			}
		}
		// Sort for deterministic output
		sort.Strings(overlaps[item.Name])
	}

	return overlaps
}

// OptimalMergeOrder returns tasks ordered to minimize cascading conflicts.
// Uses a greedy algorithm: at each step, pick the task with fewest remaining overlaps.
// After "merging" a task, its files no longer count as conflicts for remaining tasks.
func OptimalMergeOrder(items []QueueItem) []QueueItem {
	if len(items) <= 1 {
		return items
	}

	// Build file -> tasks map
	fileToTasks := make(map[string]map[string]bool)
	for _, item := range items {
		for _, f := range item.changedFiles {
			if fileToTasks[f] == nil {
				fileToTasks[f] = make(map[string]bool)
			}
			fileToTasks[f][item.Name] = true
		}
	}

	// Track which tasks are already ordered
	ordered := make([]QueueItem, 0, len(items))
	remaining := make(map[string]*QueueItem, len(items))
	for i := range items {
		remaining[items[i].Name] = &items[i]
	}

	for len(remaining) > 0 {
		// Find task with fewest remaining overlaps, preferring higher scores
		var best *QueueItem
		bestOverlaps := -1

		for _, item := range remaining {
			overlaps := countRemainingOverlaps(item, fileToTasks, remaining)

			if best == nil ||
				overlaps < bestOverlaps ||
				(overlaps == bestOverlaps && item.Score > best.Score) {
				best = item
				bestOverlaps = overlaps
			}
		}

		if best == nil {
			break
		}

		// Add to ordered list
		ordered = append(ordered, *best)
		delete(remaining, best.Name)

		// Remove this task's files from the overlap map
		// (simulating that these files are now "committed" to base)
		for _, f := range best.changedFiles {
			delete(fileToTasks[f], best.Name)
		}
	}

	return ordered
}

// countRemainingOverlaps counts how many remaining tasks share files with this task.
func countRemainingOverlaps(item *QueueItem, fileToTasks map[string]map[string]bool, remaining map[string]*QueueItem) int {
	seen := make(map[string]bool)
	for _, f := range item.changedFiles {
		for other := range fileToTasks[f] {
			if other != item.Name && remaining[other] != nil && !seen[other] {
				seen[other] = true
			}
		}
	}
	return len(seen)
}

// GetOverlappingFiles returns the files that overlap between two tasks.
func GetOverlappingFiles(a, b *QueueItem) []string {
	if a == nil || b == nil {
		return nil
	}

	aFiles := make(map[string]bool, len(a.changedFiles))
	for _, f := range a.changedFiles {
		aFiles[f] = true
	}

	var overlap []string
	for _, f := range b.changedFiles {
		if aFiles[f] {
			overlap = append(overlap, f)
		}
	}

	sort.Strings(overlap)
	return overlap
}
