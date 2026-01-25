package queue

import (
	"time"
)

// Score weights for merge queue ordering.
const (
	weightPriority     = 0.50 // Priority weight (P1=1.0, P5=0.2)
	weightAge          = 0.20 // Age weight (older = higher)
	weightConflictFree = 0.20 // Conflict-free bonus
	weightStage        = 0.10 // Stage bonus (ready=1.0, review=0.75, etc.)
)

// CalculateScore computes a merge priority score for a queue item.
// Higher scores indicate higher merge priority.
// The score is normalized to 0-1 range.
func CalculateScore(item *QueueItem, maxAgeNS int64) float64 {
	var score float64

	// Priority component: P1=1.0, P2=0.8, P3=0.6, P4=0.4, P5=0.2
	priorityScore := priorityToScore(item.Priority)
	score += weightPriority * priorityScore

	// Age component: older tasks get higher scores
	ageScore := ageToScore(item.lastHistory, maxAgeNS)
	score += weightAge * ageScore

	// Conflict-free component: tasks without conflicts get bonus
	if len(item.ConflictFiles) == 0 {
		score += weightConflictFree
	}

	// Stage component: tasks closer to ready get higher scores
	stageScore := stageToScore(item.Stage)
	score += weightStage * stageScore

	return score
}

// priorityToScore converts priority (1-5) to a 0-1 score.
// P1 (critical) = 1.0, P5 (lowest) = 0.2
func priorityToScore(priority int) float64 {
	switch priority {
	case 1:
		return 1.0
	case 2:
		return 0.8
	case 3:
		return 0.6
	case 4:
		return 0.4
	case 5:
		return 0.2
	default:
		return 0.6 // Default to P3
	}
}

// ageToScore converts task age to a 0-1 score.
// Older tasks get higher scores (up to 1 week old = 1.0).
func ageToScore(taskHistoryNS, maxAgeNS int64) float64 {
	if maxAgeNS == 0 || taskHistoryNS == 0 {
		return 0.5 // Default middle score
	}

	now := time.Now().UnixNano()
	taskAge := now - taskHistoryNS
	if taskAge <= 0 {
		return 0.0
	}

	// Normalize age to 0-1, capping at 1 week
	maxAge := int64(7 * 24 * time.Hour)
	if taskAge > maxAge {
		return 1.0
	}
	return float64(taskAge) / float64(maxAge)
}

// stageToScore converts workflow stage to a 0-1 score.
// Tasks closer to "ready" get higher scores.
func stageToScore(stage string) float64 {
	switch stage {
	case "ready":
		return 1.0
	case "review":
		return 0.75
	case "implement", "doing":
		return 0.5
	case "plan":
		return 0.25
	default:
		return 0.5 // Default
	}
}
