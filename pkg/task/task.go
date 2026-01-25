package task

import (
	"bytes"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"gopkg.in/yaml.v3"
)

// Task represents a task definition from TASK.md.
type Task struct {
	Name        string // Task name (e.g., "fix/epoch-boundary")
	Title       string // Short description
	BaseBranch  string // Branch to fork from
	FollowUp    string // Optional: task whose conversation to continue
	Model       string // Optional: override model for this task
	Reasoning   string // Optional: override reasoning (codex-only) for this task
	Schema      int    // Task schema version (0 if missing)
	Description string // Optional task description/context (not the prompt)
	Priority    int    // 1-5 (P1=critical, P5=low), default 3
}

// frontmatter is the YAML frontmatter in TASK.md.
type frontmatter struct {
	Title      string `yaml:"title"`
	BaseBranch string `yaml:"base-branch"`
	Schema     int    `yaml:"schema,omitempty"`
	FollowUp   string `yaml:"follow-up,omitempty"`
	Model      string `yaml:"model,omitempty"`
	Reasoning  string `yaml:"reasoning,omitempty"`
	Priority   int    `yaml:"priority,omitempty"`
}

// Save writes the task to .subtask/tasks/<name>/TASK.md.
func (t *Task) Save() error {
	dir := Dir(t.Name)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}

	fm := frontmatter{
		Title:      t.Title,
		BaseBranch: t.BaseBranch,
		Schema:     t.Schema,
		FollowUp:   t.FollowUp,
		Model:      t.Model,
		Reasoning:  t.Reasoning,
		Priority:   t.Priority,
	}

	var buf bytes.Buffer
	buf.WriteString("---\n")
	enc := yaml.NewEncoder(&buf)
	enc.SetIndent(2)
	if err := enc.Encode(fm); err != nil {
		return err
	}
	buf.WriteString("---\n")
	if t.Description != "" {
		buf.WriteString("\n")
		buf.WriteString(t.Description)
		if !strings.HasSuffix(t.Description, "\n") {
			buf.WriteString("\n")
		}
	}

	return os.WriteFile(Path(t.Name), buf.Bytes(), 0644)
}

// Load reads a task from .subtask/tasks/<name>/TASK.md.
func Load(name string) (*Task, error) {
	data, err := os.ReadFile(Path(name))
	if err != nil {
		if os.IsNotExist(err) {
			return nil, fmt.Errorf("task %q not found", name)
		}
		return nil, err
	}

	// Parse frontmatter
	content := string(data)
	if !strings.HasPrefix(content, "---\n") {
		return nil, fmt.Errorf("invalid TASK.md: missing frontmatter")
	}

	end := strings.Index(content[4:], "\n---")
	if end == -1 {
		return nil, fmt.Errorf("invalid TASK.md: unclosed frontmatter")
	}

	fmData := content[4 : 4+end]
	prompt := strings.TrimPrefix(content[4+end+4:], "\n")

	var fm frontmatter
	if err := yaml.Unmarshal([]byte(fmData), &fm); err != nil {
		return nil, fmt.Errorf("invalid frontmatter: %w", err)
	}

	return &Task{
		Name:        name,
		Title:       fm.Title,
		BaseBranch:  fm.BaseBranch,
		Schema:      fm.Schema,
		FollowUp:    fm.FollowUp,
		Model:       fm.Model,
		Reasoning:   fm.Reasoning,
		Description: strings.TrimSpace(prompt),
		Priority:    fm.Priority,
	}, nil
}

// List returns all task names in .subtask/tasks/.
func List() ([]string, error) {
	entries, err := os.ReadDir(TasksDir())
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}

	var names []string
	for _, e := range entries {
		if e.IsDir() {
			if _, err := os.Stat(filepath.Join(TasksDir(), e.Name(), "TASK.md")); err == nil {
				names = append(names, UnescapeName(e.Name()))
			}
		}
	}
	return names, nil
}

// Path returns the TASK.md path for this task.
func (t *Task) Path() string {
	return Path(t.Name)
}

// DefaultPriority is the default priority for tasks (middle of 1-5 scale).
const DefaultPriority = 3

// EffectivePriority returns the task's priority, defaulting to 3 if not set.
func (t *Task) EffectivePriority() int {
	if t.Priority == 0 {
		return DefaultPriority
	}
	return t.Priority
}
