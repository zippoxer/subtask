package queue

import (
	"github.com/zippoxer/subtask/pkg/git"
	"github.com/zippoxer/subtask/pkg/task"
)

// CheckMaturity checks if a task is ready to be merged.
// Returns (canMerge, reason) where reason explains why it can't merge.
func CheckMaturity(item *QueueItem, minStage string) (bool, string) {
	// Check task status
	if item.TaskStatus != task.TaskStatusOpen {
		return false, "task is " + string(item.TaskStatus)
	}

	// Check worker status
	if item.WorkerStatus == task.WorkerStatusRunning {
		return false, "worker is running"
	}

	// Check workspace exists
	if item.Workspace == "" {
		return false, "no workspace assigned"
	}

	// Check stage requirement
	if minStage != "" && !stageReached(item.Stage, minStage) {
		return false, "stage is " + item.Stage + ", requires " + minStage
	}

	// Check workspace is clean
	clean, err := git.IsClean(item.Workspace)
	if err != nil {
		return false, "cannot check workspace: " + err.Error()
	}
	if !clean {
		return false, "workspace has uncommitted changes"
	}

	// Check for conflicts with base branch
	if len(item.ConflictFiles) > 0 {
		return false, "has conflicts with " + item.BaseBranch
	}

	// Check for commits to merge
	if !hasCommits(item) {
		return false, "no commits to merge"
	}

	return true, ""
}

// stageReached checks if current stage has reached the minimum required stage.
func stageReached(current, min string) bool {
	stages := []string{"plan", "implement", "doing", "review", "ready"}

	currentIdx := -1
	minIdx := -1
	for i, s := range stages {
		if s == current {
			currentIdx = i
		}
		if s == min {
			minIdx = i
		}
	}

	// If stage not found, be lenient
	if currentIdx == -1 || minIdx == -1 {
		return true
	}

	return currentIdx >= minIdx
}

// hasCommits checks if the task branch has commits to merge.
func hasCommits(item *QueueItem) bool {
	if item.Workspace == "" || item.BaseBranch == "" {
		return false
	}

	// Get merge base
	base, err := git.MergeBase(item.Workspace, item.BaseBranch, "HEAD")
	if err != nil {
		return false
	}

	// Check if there are commits between merge base and HEAD
	subjects, err := git.GetCommitSubjects(item.Workspace, base)
	if err != nil {
		return false
	}

	return len(subjects) > 0
}

// MaturityStatus provides detailed maturity information.
type MaturityStatus struct {
	CanMerge       bool
	Reason         string
	WorkspaceClean bool
	HasCommits     bool
	StageReached   bool
	HasConflicts   bool
}

// GetMaturityStatus returns detailed maturity information for a queue item.
func GetMaturityStatus(item *QueueItem, minStage string) MaturityStatus {
	status := MaturityStatus{}

	// Task must be open
	if item.TaskStatus != task.TaskStatusOpen {
		status.Reason = "task is " + string(item.TaskStatus)
		return status
	}

	// Worker must not be running
	if item.WorkerStatus == task.WorkerStatusRunning {
		status.Reason = "worker is running"
		return status
	}

	// Check workspace
	if item.Workspace == "" {
		status.Reason = "no workspace assigned"
		return status
	}

	// Check stage
	status.StageReached = stageReached(item.Stage, minStage)
	if minStage != "" && !status.StageReached {
		status.Reason = "stage is " + item.Stage + ", requires " + minStage
	}

	// Check workspace cleanliness
	clean, err := git.IsClean(item.Workspace)
	if err == nil {
		status.WorkspaceClean = clean
		if !clean && status.Reason == "" {
			status.Reason = "workspace has uncommitted changes"
		}
	}

	// Check conflicts
	status.HasConflicts = len(item.ConflictFiles) > 0
	if status.HasConflicts && status.Reason == "" {
		status.Reason = "has conflicts with " + item.BaseBranch
	}

	// Check commits
	status.HasCommits = hasCommits(item)
	if !status.HasCommits && status.Reason == "" {
		status.Reason = "no commits to merge"
	}

	// Determine overall merge readiness
	status.CanMerge = status.WorkspaceClean &&
		status.HasCommits &&
		status.StageReached &&
		!status.HasConflicts &&
		item.TaskStatus == task.TaskStatusOpen &&
		item.WorkerStatus != task.WorkerStatusRunning

	return status
}
