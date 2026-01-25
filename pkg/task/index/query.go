package index

import (
	"context"
	"database/sql"
	"fmt"
	"time"

	"github.com/zippoxer/subtask/pkg/task"
)

// ListItem is a task summary record suitable for list output.
type ListItem struct {
	Name       string
	Title      string
	FollowUp   string
	BaseBranch string
	Priority   int

	TaskStatus   task.TaskStatus
	WorkerStatus task.WorkerStatus
	Stage        string

	Workspace string
	StartedAt time.Time
	LastError string

	LastHistory time.Time
	LastActive  time.Time
	ToolCalls   int

	LastRunDurationMS int

	ProgressDone  int
	ProgressTotal int

	LinesAdded    int
	LinesRemoved  int
	CommitsBehind int

	IntegratedReason string
}

// Record is the cached file-backed data for a single task.
type Record struct {
	Task *task.Task

	TaskStatus   task.TaskStatus
	WorkerStatus task.WorkerStatus
	Stage        string
	Priority     int

	State        *task.State
	ProgressMeta *task.Progress

	LastHistory time.Time

	ProgressDone  int
	ProgressTotal int

	LastRunDurationMS int

	LinesAdded        int
	LinesRemoved      int
	CommitsBehind     int
	ConflictFilesJSON string
	IntegratedReason  string
}

func (i *Index) ListAll(ctx context.Context) ([]ListItem, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	const q = `
SELECT
	name, title, follow_up, base_branch, priority,
	task_status, worker_status, stage,
	workspace, started_at_ns, last_error,
	last_history_ns,
	last_active_ns, tool_calls,
	last_run_duration_ms,
	progress_done, progress_total,
	git_lines_added, git_lines_removed, git_commits_behind,
	git_integrated_reason
FROM tasks
ORDER BY last_history_ns DESC, name ASC;
`
	return i.queryList(ctx, q)
}

func (i *Index) ListOpen(ctx context.Context) ([]ListItem, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	const q = `
SELECT
	name, title, follow_up, base_branch, priority,
	task_status, worker_status, stage,
	workspace, started_at_ns, last_error,
	last_history_ns,
	last_active_ns, tool_calls,
	last_run_duration_ms,
	progress_done, progress_total,
	git_lines_added, git_lines_removed, git_commits_behind,
	git_integrated_reason
FROM tasks
WHERE task_status != 'closed'
ORDER BY last_history_ns DESC, name ASC;
`
	return i.queryList(ctx, q)
}

func (i *Index) ListClosed(ctx context.Context) ([]ListItem, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	const q = `
SELECT
	name, title, follow_up, base_branch, priority,
	task_status, worker_status, stage,
	workspace, started_at_ns, last_error,
	last_history_ns,
	last_active_ns, tool_calls,
	last_run_duration_ms,
	progress_done, progress_total,
	git_lines_added, git_lines_removed, git_commits_behind,
	git_integrated_reason
FROM tasks
WHERE task_status = 'closed'
ORDER BY last_history_ns DESC, name ASC;
`
	return i.queryList(ctx, q)
}

func (i *Index) queryList(ctx context.Context, q string) ([]ListItem, error) {
	rows, err := i.db.QueryContext(ctx, q)
	if err != nil {
		return nil, fmt.Errorf("index list: %w", err)
	}
	defer rows.Close()

	var out []ListItem
	for rows.Next() {
		var (
			name, title, followUp, baseBranch string
			priority                          int
			taskStatus, workerStatus, stage   string
			workspace                         string
			startedAtNS                       int64
			lastError                         sql.NullString
			lastHistoryNS                     int64
			lastActiveNS                      int64
			toolCalls                         int
			lastRunDurationMS                 int
			progressDone, progressTotal       int
			linesAdded, linesRemoved          sql.NullInt64
			commitsBehind                     sql.NullInt64
			integratedReason                  sql.NullString
		)
		if err := rows.Scan(
			&name, &title, &followUp, &baseBranch, &priority,
			&taskStatus, &workerStatus, &stage,
			&workspace, &startedAtNS, &lastError,
			&lastHistoryNS,
			&lastActiveNS, &toolCalls,
			&lastRunDurationMS,
			&progressDone, &progressTotal,
			&linesAdded, &linesRemoved, &commitsBehind,
			&integratedReason,
		); err != nil {
			return nil, fmt.Errorf("index list: scan: %w", err)
		}

		item := ListItem{
			Name:              name,
			Title:             title,
			FollowUp:          followUp,
			BaseBranch:        baseBranch,
			Priority:          priority,
			TaskStatus:        task.TaskStatus(taskStatus),
			WorkerStatus:      task.ParseWorkerStatus(workerStatus),
			Stage:             stage,
			Workspace:         workspace,
			StartedAt:         timeFromNS(startedAtNS),
			LastHistory:       timeFromNS(lastHistoryNS),
			LastActive:        timeFromNS(lastActiveNS),
			ToolCalls:         toolCalls,
			LastRunDurationMS: lastRunDurationMS,
			ProgressDone:      progressDone,
			ProgressTotal:     progressTotal,
			LinesAdded:        intOrZero(linesAdded),
			LinesRemoved:      intOrZero(linesRemoved),
			CommitsBehind:     intOrZero(commitsBehind),
		}
		if lastError.Valid {
			item.LastError = lastError.String
		}
		if integratedReason.Valid {
			item.IntegratedReason = integratedReason.String
		}

		out = append(out, item)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("index list: rows: %w", err)
	}
	return out, nil
}

func (i *Index) Get(ctx context.Context, taskName string) (Record, bool, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	const q = `
SELECT
	name, title, base_branch, follow_up, model, reasoning, description,
	task_schema, priority, task_status, worker_status, stage,
	workspace, started_at_ns, supervisor_pid, last_error,
	last_history_ns,
	tool_calls, last_active_ns,
	last_run_duration_ms,
	progress_done, progress_total,
	git_lines_added, git_lines_removed, git_commits_behind,
	git_conflict_files_json,
	git_integrated_reason
FROM tasks
WHERE name = ?;
`

	var (
		name, title, baseBranch, followUp, model, reasoning, description string
		taskSchema                                                       int
		priority                                                         int
		taskStatus, workerStatus, stage                                  string
		workspace                                                        string
		startedAtNS                                                      int64
		supervisorPID                                                    int
		lastError                                                        sql.NullString
		lastHistoryNS                                                    int64
		toolCalls                                                        int
		lastActiveNS                                                     int64
		lastRunDurationMS                                                int
		progressDone, progressTotal                                      int
		linesAdded, linesRemoved, commitsBehind                          sql.NullInt64
		conflictFilesJSON                                                sql.NullString
		integratedReason                                                 sql.NullString
	)

	err := i.db.QueryRowContext(ctx, q, taskName).Scan(
		&name, &title, &baseBranch, &followUp, &model, &reasoning, &description,
		&taskSchema, &priority, &taskStatus, &workerStatus, &stage,
		&workspace, &startedAtNS, &supervisorPID, &lastError,
		&lastHistoryNS,
		&toolCalls, &lastActiveNS,
		&lastRunDurationMS,
		&progressDone, &progressTotal,
		&linesAdded, &linesRemoved, &commitsBehind,
		&conflictFilesJSON,
		&integratedReason,
	)
	if err != nil {
		if err == sql.ErrNoRows {
			return Record{}, false, nil
		}
		return Record{}, false, fmt.Errorf("index get: %w", err)
	}

	rec := Record{
		Task: &task.Task{
			Name:        name,
			Title:       title,
			BaseBranch:  baseBranch,
			FollowUp:    followUp,
			Model:       model,
			Reasoning:   reasoning,
			Schema:      taskSchema,
			Description: description,
			Priority:    priority,
		},
		TaskStatus:        task.TaskStatus(taskStatus),
		WorkerStatus:      task.ParseWorkerStatus(workerStatus),
		Stage:             stage,
		Priority:          priority,
		LastHistory:       timeFromNS(lastHistoryNS),
		ProgressDone:      progressDone,
		ProgressTotal:     progressTotal,
		LastRunDurationMS: lastRunDurationMS,
		LinesAdded:        intOrZero(linesAdded),
		LinesRemoved:      intOrZero(linesRemoved),
		CommitsBehind:     intOrZero(commitsBehind),
	}

	st := &task.State{
		Workspace:     workspace,
		SupervisorPID: supervisorPID,
		StartedAt:     timeFromNS(startedAtNS),
	}
	if lastError.Valid {
		st.LastError = lastError.String
	}
	rec.State = st

	if toolCalls != 0 || lastActiveNS != 0 {
		rec.ProgressMeta = &task.Progress{
			ToolCalls:  toolCalls,
			LastActive: timeFromNS(lastActiveNS),
		}
	}
	if conflictFilesJSON.Valid {
		rec.ConflictFilesJSON = conflictFilesJSON.String
	}
	if integratedReason.Valid {
		rec.IntegratedReason = integratedReason.String
	}

	return rec, true, nil
}

func timeFromNS(ns int64) time.Time {
	if ns == 0 {
		return time.Time{}
	}
	return time.Unix(0, ns)
}

func intOrZero(n sql.NullInt64) int {
	if !n.Valid {
		return 0
	}
	return int(n.Int64)
}
