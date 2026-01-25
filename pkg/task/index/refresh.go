package index

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/zippoxer/subtask/pkg/logging"
	"github.com/zippoxer/subtask/pkg/task"
	"github.com/zippoxer/subtask/pkg/task/history"
	"github.com/zippoxer/subtask/pkg/task/migrate"
)

// RefreshPolicy controls what work Refresh performs.
// File-backed fields are always refreshed; git refresh is optional.
type RefreshPolicy struct {
	Git GitPolicy
}

type taskUpsert struct {
	name string
	row  taskRow
}

// Refresh updates the index from task files on disk.
//
// It is safe to call frequently: when no files changed, Refresh performs no DB writes.
func (i *Index) Refresh(ctx context.Context, policy RefreshPolicy) error {
	if ctx == nil {
		ctx = context.Background()
	}

	debug := logging.DebugEnabled()
	var start time.Time
	var step time.Time
	if debug {
		start = time.Now()
		logging.Debug("refresh", fmt.Sprintf("index.Refresh start git={mode:%s includeIntegration:%t includeConflicts:%t ttl:%s tasks:%d}",
			gitModeString(policy.Git.Mode), policy.Git.IncludeIntegration, policy.Git.IncludeConflicts, policy.Git.TTL, len(policy.Git.Tasks)))
	}

	if debug {
		step = time.Now()
	}
	diskTasks, err := task.List()
	if err != nil {
		return err
	}
	if debug {
		logging.Debug("refresh", fmt.Sprintf("task.List n=%d (%s)", len(diskTasks), time.Since(step).Round(time.Millisecond)))
	}

	if debug {
		step = time.Now()
	}
	existing, err := i.loadFilesSigs(ctx)
	if err != nil {
		return err
	}
	if debug {
		logging.Debug("refresh", fmt.Sprintf("index.loadFilesSigs n=%d (%s)", len(existing), time.Since(step).Round(time.Millisecond)))
	}

	diskSet := make(map[string]struct{}, len(diskTasks))
	for _, name := range diskTasks {
		diskSet[name] = struct{}{}
	}

	var toDelete []string
	for name := range existing {
		if _, ok := diskSet[name]; !ok {
			toDelete = append(toDelete, name)
		}
	}
	sort.Strings(toDelete)

	var invalidateGit []string
	var toUpsert []taskUpsert
	upserted := make(map[string]struct{})

	var sigTotal time.Duration
	var buildTotal time.Duration
	var diskLoopErrors int

	for _, name := range diskTasks {
		if debug {
			step = time.Now()
		}
		sig, err := filesSigForTask(name)
		if err != nil {
			diskLoopErrors++
			return err
		}
		if debug {
			sigTotal += time.Since(step)
		}
		if prev, ok := existing[name]; ok {
			if prev == sig {
				continue
			}
			if shouldInvalidateGit(prev, sig) {
				invalidateGit = append(invalidateGit, name)
			}
		}

		if debug {
			step = time.Now()
		}
		row, ok, err := buildRowFromDisk(name, sig)
		if err != nil {
			diskLoopErrors++
			return err
		}
		if debug {
			buildTotal += time.Since(step)
		}
		if !ok {
			if _, exists := existing[name]; exists {
				toDelete = append(toDelete, name)
			}
			continue
		}

		toUpsert = append(toUpsert, taskUpsert{name: name, row: row})
		upserted[name] = struct{}{}
	}

	// Staleness is not reflected by mtimes; refresh any tasks whose supervisor PID is now dead.
	if debug {
		step = time.Now()
	}
	staleNames, err := i.staleSupervisorTasks(ctx)
	if err != nil {
		return err
	}
	if debug {
		logging.Debug("refresh", fmt.Sprintf("index.staleSupervisorTasks n=%d (%s)", len(staleNames), time.Since(step).Round(time.Millisecond)))
	}
	for _, name := range staleNames {
		if _, ok := diskSet[name]; !ok {
			continue
		}
		if _, ok := upserted[name]; ok {
			continue
		}
		sig, err := filesSigForTask(name)
		if err != nil {
			return err
		}
		row, ok, err := buildRowFromDisk(name, sig)
		if err != nil {
			return err
		}
		if !ok {
			if _, exists := existing[name]; exists {
				toDelete = append(toDelete, name)
			}
			continue
		}
		toUpsert = append(toUpsert, taskUpsert{name: name, row: row})
		upserted[name] = struct{}{}
	}

	if len(toDelete) == 0 && len(toUpsert) == 0 {
		if debug {
			logging.Debug("refresh", fmt.Sprintf("disk unchanged sigTotal=%s buildTotal=%s", sigTotal.Round(time.Millisecond), buildTotal.Round(time.Millisecond)))
		}
		var gitStart time.Time
		if debug {
			gitStart = time.Now()
		}
		err := i.refreshGit(ctx, policy.Git)
		if debug {
			logging.Debug("refresh", fmt.Sprintf("refreshGit (%s)", time.Since(gitStart).Round(time.Millisecond)))
			logging.Debug("refresh", fmt.Sprintf("index.Refresh done (%s)", time.Since(start).Round(time.Millisecond)))
		}
		return err
	}

	if debug {
		step = time.Now()
	}
	tx, err := i.db.BeginTx(ctx, nil)
	if err != nil {
		return fmt.Errorf("index refresh: begin tx: %w", err)
	}
	defer tx.Rollback()

	if len(toDelete) > 0 {
		if err := deleteTasks(ctx, tx, toDelete); err != nil {
			return err
		}
	}
	if len(toUpsert) > 0 {
		if err := upsertTasks(ctx, tx, toUpsert); err != nil {
			return err
		}
	}
	if len(invalidateGit) > 0 {
		if err := invalidateGitCache(ctx, tx, invalidateGit); err != nil {
			return err
		}
	}
	if err := tx.Commit(); err != nil {
		return fmt.Errorf("index refresh: commit: %w", err)
	}
	if debug {
		logging.Debug("refresh", fmt.Sprintf("db writes delete=%d upsert=%d invalidateGit=%d (%s)", len(toDelete), len(toUpsert), len(invalidateGit), time.Since(step).Round(time.Millisecond)))
	}

	var gitStart time.Time
	if debug {
		gitStart = time.Now()
	}
	err = i.refreshGit(ctx, policy.Git)
	if debug {
		logging.Debug("refresh", fmt.Sprintf("refreshGit (%s)", time.Since(gitStart).Round(time.Millisecond)))
		logging.Debug("refresh", fmt.Sprintf("index.Refresh done (%s) tasks=%d upsert=%d delete=%d invalidateGit=%d staleSupervisor=%d sigTotal=%s buildTotal=%s",
			time.Since(start).Round(time.Millisecond), len(diskTasks), len(toUpsert), len(toDelete), len(invalidateGit), len(staleNames), sigTotal.Round(time.Millisecond), buildTotal.Round(time.Millisecond)))
		_ = diskLoopErrors
	}
	return err
}

func gitModeString(m GitMode) string {
	switch m {
	case GitNone:
		return "GitNone"
	case GitOpenOnly:
		return "GitOpenOnly"
	case GitAll:
		return "GitAll"
	case GitTasks:
		return "GitTasks"
	default:
		return fmt.Sprintf("GitMode(%d)", int(m))
	}
}

type taskRow struct {
	// Identity
	name string

	// Task fields (TASK.md)
	title       string
	baseBranch  string
	baseCommit  string
	followUp    string
	model       string
	reasoning   string
	description string
	taskSchema  int
	priority    int

	// Durable state (history.jsonl)
	taskStatus      string
	stage           string
	lastHistoryNS   int64
	lastRunDuration int

	// Runtime (state.json)
	workspace     string
	startedAtNS   int64
	supervisorPID int
	lastError     string
	workerStatus  string

	// Progress meta (progress.json)
	toolCalls    int
	lastActiveNS int64

	// Progress steps summary (PROGRESS.json)
	progressDone  int
	progressTotal int

	// Derived
	statusRank int
	filesSig   string
}

func buildRowFromDisk(taskName, filesSig string) (taskRow, bool, error) {
	var row taskRow
	row.name = taskName
	row.filesSig = filesSig

	t, err := task.Load(taskName)
	if err != nil {
		return taskRow{}, false, nil
	}

	// One-time per-task migration driven by TASK.md schema.
	if t.Schema < migrate.CurrentSchema {
		if err := migrate.EnsureSchema(taskName); err != nil {
			return taskRow{}, false, err
		}
		t, err = task.Load(taskName)
		if err != nil {
			return taskRow{}, false, nil
		}
		// Migration updates TASK.md/history.jsonl; refresh signature for DB consistency.
		if sig2, err := filesSigForTask(taskName); err == nil {
			row.filesSig = sig2
		}
	}

	row.title = t.Title
	row.baseBranch = t.BaseBranch
	row.followUp = t.FollowUp
	row.model = t.Model
	row.reasoning = t.Reasoning
	row.description = t.Description
	row.taskSchema = t.Schema
	row.priority = t.EffectivePriority()

	// Durable state from history.
	tail, _ := history.Tail(taskName)
	row.taskStatus = string(tail.TaskStatus)
	row.stage = tail.Stage
	row.lastHistoryNS = tail.LastTS.UnixNano()
	row.lastRunDuration = tail.LastRunDurationMS
	row.baseCommit = tail.BaseCommit

	// Runtime state.
	state, err := task.LoadState(taskName)
	if err != nil {
		state = nil
	}
	if state != nil && state.IsStale() {
		if fixed, err := fixStaleState(taskName); err == nil && fixed != nil {
			state = fixed
			// state.json rewrite changes signature; refresh for DB consistency.
			if sig2, err := filesSigForTask(taskName); err == nil {
				row.filesSig = sig2
			}
		}
	}

	if state != nil {
		row.workspace = state.Workspace
		row.startedAtNS = state.StartedAt.UnixNano()
		row.supervisorPID = state.SupervisorPID
		row.lastError = state.LastError
	}

	// Worker status: local runtime for "running", history for last outcome.
	row.workerStatus = string(deriveWorkerStatus(state, tail))

	row.statusRank = statusRank(task.WorkerStatus(row.workerStatus), task.TaskStatus(row.taskStatus))

	meta, err := task.LoadProgress(taskName)
	if err != nil {
		meta = nil
	}
	if meta != nil {
		row.toolCalls = meta.ToolCalls
		row.lastActiveNS = meta.LastActive.UnixNano()
	}

	steps := task.LoadProgressSteps(taskName)
	done, total := task.CountProgressSteps(steps)
	row.progressDone = done
	row.progressTotal = total

	return row, true, nil
}

func deriveWorkerStatus(state *task.State, tail history.TailInfo) task.WorkerStatus {
	if state != nil && state.SupervisorPID != 0 && !state.IsStale() {
		return task.WorkerStatusRunning
	}
	if state != nil && strings.TrimSpace(state.LastError) != "" {
		return task.WorkerStatusError
	}
	switch strings.TrimSpace(tail.LastRunOutcome) {
	case "error":
		return task.WorkerStatusError
	case "replied":
		return task.WorkerStatusReplied
	default:
		return task.WorkerStatusNotStarted
	}
}

func statusRank(worker task.WorkerStatus, ts task.TaskStatus) int {
	workerRank := 3
	switch worker {
	case task.WorkerStatusRunning:
		workerRank = 0
	case task.WorkerStatusReplied:
		workerRank = 1
	case task.WorkerStatusError:
		workerRank = 2
	}

	taskRank := 0
	switch ts {
	case task.TaskStatusOpen:
		taskRank = 0
	case task.TaskStatusMerged:
		taskRank = 1
	case task.TaskStatusClosed:
		taskRank = 2
	default:
		taskRank = 3
	}

	return workerRank*10 + taskRank
}

func fixStaleState(taskName string) (*task.State, error) {
	var updated *task.State
	locked, err := task.TryWithLock(taskName, func() error {
		st, err := task.LoadState(taskName)
		if err != nil || st == nil {
			return err
		}
		if !st.IsStale() {
			updated = st
			return nil
		}

		st.SupervisorPID = 0
		st.SupervisorPGID = 0
		st.StartedAt = time.Time{}
		if strings.TrimSpace(st.LastError) == "" {
			st.LastError = "supervisor process died"
		}
		if err := st.Save(taskName); err != nil {
			return err
		}
		updated = st
		return nil
	})
	if err != nil {
		return nil, err
	}
	if !locked {
		return nil, nil
	}
	return updated, nil
}

func (i *Index) staleSupervisorTasks(ctx context.Context) ([]string, error) {
	rows, err := i.db.QueryContext(ctx, "SELECT name, supervisor_pid FROM tasks WHERE supervisor_pid != 0;")
	if err != nil {
		return nil, fmt.Errorf("index refresh: query running tasks: %w", err)
	}
	defer rows.Close()

	var out []string
	for rows.Next() {
		var name string
		var pid int
		if err := rows.Scan(&name, &pid); err != nil {
			return nil, fmt.Errorf("index refresh: scan running tasks: %w", err)
		}
		st := &task.State{SupervisorPID: pid}
		if st.IsStale() {
			out = append(out, name)
		}
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("index refresh: iterate running tasks: %w", err)
	}
	return out, nil
}

func filesSigForTask(taskName string) (sig string, _ error) {
	// Encode (mtime_ns,size) for the tracked files. Missing files are "0:0".
	parts := []struct {
		key  string
		path string
	}{
		{key: "task", path: task.Path(taskName)},
		{key: "history", path: task.HistoryPath(taskName)},
		{key: "state", path: task.StatePath(taskName)},
		{key: "progress", path: filepath.Join(task.InternalDir(), task.EscapeName(taskName), "progress.json")},
		{key: "steps", path: filepath.Join(task.Dir(taskName), "PROGRESS.json")},
	}

	var b strings.Builder
	for idx, p := range parts {
		mtimeNS, size := fileSig(p.path)
		if idx > 0 {
			b.WriteByte(';')
		}
		b.WriteString(p.key)
		b.WriteByte('=')
		b.WriteString(strconv.FormatInt(mtimeNS, 10))
		b.WriteByte(':')
		b.WriteString(strconv.FormatInt(size, 10))
	}

	return b.String(), nil
}

func fileSig(path string) (mtimeNS int64, size int64) {
	st, err := os.Stat(path)
	if err != nil {
		return 0, 0
	}
	return st.ModTime().UnixNano(), st.Size()
}

func (i *Index) loadFilesSigs(ctx context.Context) (map[string]string, error) {
	rows, err := i.db.QueryContext(ctx, "SELECT name, files_sig FROM tasks;")
	if err != nil {
		return nil, fmt.Errorf("index refresh: query existing sigs: %w", err)
	}
	defer rows.Close()

	out := make(map[string]string)
	for rows.Next() {
		var name, sig string
		if err := rows.Scan(&name, &sig); err != nil {
			return nil, fmt.Errorf("index refresh: scan sigs: %w", err)
		}
		out[name] = sig
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("index refresh: iterate sigs: %w", err)
	}
	return out, nil
}

func deleteTasks(ctx context.Context, tx *sql.Tx, names []string) error {
	stmt, err := tx.PrepareContext(ctx, "DELETE FROM tasks WHERE name = ?;")
	if err != nil {
		return fmt.Errorf("index refresh: prepare delete: %w", err)
	}
	defer stmt.Close()

	for _, name := range names {
		if _, err := stmt.ExecContext(ctx, name); err != nil {
			return fmt.Errorf("index refresh: delete %q: %w", name, err)
		}
	}
	return nil
}

func upsertTasks(ctx context.Context, tx *sql.Tx, rows []taskUpsert) error {
	const q = `
INSERT INTO tasks (
	name,
	title, base_branch, base_commit, follow_up, model, reasoning, description,
	task_schema, task_status, worker_status, stage, workspace, started_at_ns, supervisor_pid, last_error,
	last_history_ns,
	tool_calls, last_active_ns,
	last_run_duration_ms,
	progress_done, progress_total,
	status_rank,
	priority,
	files_sig
) VALUES (
	?, ?, ?, ?, ?, ?, ?, ?,
	?, ?, ?, ?, ?, ?, ?, ?,
	?,
	?, ?,
	?,
	?, ?,
	?,
	?,
	?
)
ON CONFLICT(name) DO UPDATE SET
	title=excluded.title,
	base_branch=excluded.base_branch,
	base_commit=excluded.base_commit,
	follow_up=excluded.follow_up,
	model=excluded.model,
	reasoning=excluded.reasoning,
	description=excluded.description,
	task_schema=excluded.task_schema,
	task_status=excluded.task_status,
	worker_status=excluded.worker_status,
	stage=excluded.stage,
	workspace=excluded.workspace,
	started_at_ns=excluded.started_at_ns,
	supervisor_pid=excluded.supervisor_pid,
	last_error=excluded.last_error,
	last_history_ns=excluded.last_history_ns,
	tool_calls=excluded.tool_calls,
	last_active_ns=excluded.last_active_ns,
	last_run_duration_ms=excluded.last_run_duration_ms,
	progress_done=excluded.progress_done,
	progress_total=excluded.progress_total,
	status_rank=excluded.status_rank,
	priority=excluded.priority,
	files_sig=excluded.files_sig;
`

	stmt, err := tx.PrepareContext(ctx, q)
	if err != nil {
		return fmt.Errorf("index refresh: prepare upsert: %w", err)
	}
	defer stmt.Close()

	for _, r := range rows {
		row := r.row
		if _, err := stmt.ExecContext(ctx,
			row.name,
			row.title, row.baseBranch, row.baseCommit, row.followUp, row.model, row.reasoning, row.description,
			row.taskSchema, row.taskStatus, row.workerStatus, row.stage, row.workspace, row.startedAtNS, row.supervisorPID, nullableString(row.lastError),
			row.lastHistoryNS,
			row.toolCalls, row.lastActiveNS,
			row.lastRunDuration,
			row.progressDone, row.progressTotal,
			row.statusRank,
			row.priority,
			row.filesSig,
		); err != nil {
			return fmt.Errorf("index refresh: upsert %q: %w", row.name, err)
		}
	}
	return nil
}

func nullableString(s string) any {
	if strings.TrimSpace(s) == "" {
		return nil
	}
	return s
}

func shouldInvalidateGit(prevSig, nextSig string) bool {
	// Git stats depend on TASK.md (base branch) and state.json (workspace).
	// Progress-only/history-only changes should not invalidate git cache.
	return sigPart(prevSig, "task") != sigPart(nextSig, "task") ||
		sigPart(prevSig, "state") != sigPart(nextSig, "state")
}

func sigPart(sig, key string) string {
	prefix := key + "="
	for _, part := range strings.Split(sig, ";") {
		if strings.HasPrefix(part, prefix) {
			return strings.TrimPrefix(part, prefix)
		}
	}
	return ""
}

func invalidateGitCache(ctx context.Context, tx *sql.Tx, names []string) error {
	stmt, err := tx.PrepareContext(ctx, `
UPDATE tasks SET
	git_lines_added = NULL,
	git_lines_removed = NULL,
	git_commits_behind = NULL,
	git_conflict_files_json = NULL,
	git_integrated_reason = NULL,
	git_integrated_branch_head = NULL,
	git_integrated_target_head = NULL,
	git_integrated_checked_at_ns = NULL,
	git_patch_id = NULL,
	git_base_ref = NULL,
	git_target_ref = NULL,
	git_computed_at_ns = NULL,
	git_error = NULL
WHERE name = ?;`)
	if err != nil {
		return fmt.Errorf("index refresh: prepare git invalidate: %w", err)
	}
	defer stmt.Close()

	for _, name := range names {
		if _, err := stmt.ExecContext(ctx, name); err != nil {
			return fmt.Errorf("index refresh: git invalidate %q: %w", name, err)
		}
	}
	return nil
}
