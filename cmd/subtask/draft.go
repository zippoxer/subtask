package main

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/zippoxer/subtask/pkg/git"
	"github.com/zippoxer/subtask/pkg/render"
	"github.com/zippoxer/subtask/pkg/task"
	"github.com/zippoxer/subtask/pkg/task/history"
	"github.com/zippoxer/subtask/pkg/workflow"
	"github.com/zippoxer/subtask/pkg/workspace"
)

// DraftCmd implements 'subtask draft'.
type DraftCmd struct {
	Task        string `arg:"" help:"Task name (e.g., fix/epoch-boundary)"`
	Description string `arg:"" optional:"" help:"Task description (or use stdin)"`
	Base        string `name:"base-branch" required:"" help:"Base branch"`
	Title       string `required:"" help:"Short description"`
	Priority    int    `default:"3" help:"Priority 1-5 (P1=critical, P5=low, default P3)"`
	Model       string `help:"Default model for this task (overrides project config)"`
	Reasoning   string `help:"Default reasoning for this task (codex-only; overrides project config)"`
	Workflow    string `help:"Workflow template to use (e.g., collaborative)"`
	FollowUp    string `name:"follow-up" help:"Task whose conversation to continue"`
}

// Run executes the draft command.
func (c *DraftCmd) Run() error {
	// Read description from stdin if not provided as arg
	description := c.Description
	if description == "" {
		description = readStdinForDraft()
	}

	if description == "" {
		return fmt.Errorf("description is required\n\n" +
			"Provide description as argument or via stdin (heredoc/pipe)")
	}

	// Check if task already exists
	if _, err := task.Load(c.Task); err == nil {
		return fmt.Errorf("task %q already exists", c.Task)
	}

	if strings.Contains(c.Task, "--") {
		return fmt.Errorf("task name cannot contain \"--\" (used for path escaping)")
	}

	// Load workflow (default if not specified)
	workflowName := c.Workflow
	if workflowName == "" {
		workflowName = "default"
	}
	wf, err := workflow.Load(workflow.TemplateDir(workflowName))
	if err != nil {
		return fmt.Errorf("workflow %q: %w", workflowName, err)
	}

	// Create task
	if err := workspace.ValidateReasoningLevel(c.Reasoning); err != nil {
		return err
	}

	// Validate and normalize priority (0 means use default)
	priority := c.Priority
	if priority == 0 {
		priority = task.DefaultPriority
	}
	if priority < 1 || priority > 5 {
		return fmt.Errorf("priority must be 1-5 (P1=critical, P5=low), got %d", c.Priority)
	}

	t := &task.Task{
		Name:        c.Task,
		Title:       c.Title,
		BaseBranch:  c.Base,
		Description: description,
		FollowUp:    c.FollowUp,
		Model:       c.Model,
		Reasoning:   c.Reasoning,
		Schema:      1,
		Priority:    priority,
	}

	if err := t.Save(); err != nil {
		return fmt.Errorf("failed to save task: %w", err)
	}

	// Copy workflow files to task folder
	if err := workflow.CopyToTask(workflowName, c.Task); err != nil {
		return fmt.Errorf("failed to copy workflow: %w", err)
	}

	// Capture base branch commit for staleness/conflict heuristics.
	repoRoot := task.ProjectRoot()

	// Local-first: capture from the local base branch only. If users want fresh remote
	// state they can run `git fetch` themselves before drafting.
	baseRef := c.Base

	baseCommit, err := git.Output(repoRoot, "rev-parse", baseRef)
	if err != nil {
		return fmt.Errorf("failed to resolve base branch %q: %w", baseRef, err)
	}

	openedData, _ := json.Marshal(map[string]any{
		"reason":      "draft",
		"branch":      c.Task,
		"base_branch": c.Base,
		"workflow":    wf.Name,
		"title":       c.Title,
		"priority":    priority,
		"follow_up":   c.FollowUp,
		"model":       c.Model,
		"reasoning":   c.Reasoning,
		"base_ref":    baseRef,
		"base_commit": baseCommit,
	})
	stageData, _ := json.Marshal(map[string]any{
		"from": "",
		"to":   wf.FirstStage(),
	})
	if err := history.WriteAll(c.Task, []history.Event{
		{TS: time.Now().UTC(), Type: "task.opened", Data: openedData},
		{TS: time.Now().UTC(), Type: "stage.changed", Data: stageData},
	}); err != nil {
		return fmt.Errorf("failed to write history: %w", err)
	}

	// Output
	printSuccess(fmt.Sprintf("Drafted task: %s", c.Task))
	fmt.Printf("Task folder: %s/\n", filepath.ToSlash(task.Dir(c.Task)))
	fmt.Println("  Files here are shared with worker (PLAN.md, notes, etc.)")
	fmt.Println()

	// Show lead instructions from workflow
	if wf.Instructions.Lead != "" {
		printSection("Workflow: " + wf.Name)
		printSectionContent(wf.Instructions.Lead)
	}

	// Show current stage
	printSection("Stage: " + wf.FirstStage())
	fmt.Println(render.FormatStageProgression(wf.StageNames(), wf.FirstStage()))
	fmt.Println()

	stage := wf.GetStage(wf.FirstStage())
	if stage != nil && stage.Instructions != "" {
		lines := strings.Split(strings.TrimSpace(stage.Instructions), "\n")
		for _, line := range lines {
			line = strings.ReplaceAll(line, "<task>", c.Task)
			fmt.Println(line)
		}
	}

	// Show how to run
	printSection("Usage")
	fmt.Printf("subtask send %s \"<prompt>\"\n", c.Task)

	return nil
}

// readStdinForDraft reads from stdin if data is piped/heredoc.
func readStdinForDraft() string {
	fi, err := os.Stdin.Stat()
	if err != nil {
		return ""
	}
	// Only read if stdin is a pipe or has data (not a terminal)
	mode := fi.Mode()
	if (mode&os.ModeCharDevice) != 0 || (mode&os.ModeNamedPipe) == 0 && fi.Size() == 0 {
		return ""
	}
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(data))
}
