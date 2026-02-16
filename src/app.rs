use std::collections::{HashMap, HashSet};

use crate::config::Config;
use crate::state::{self, ClaudeInstance};
use crate::tmux;

pub struct App {
    pub instances: Vec<ClaudeInstance>,
    pub selected: usize,
    pub filter: String,
    pub filtered_indices: Vec<usize>,
    pub preview: String,
    pub should_quit: bool,
    pub jump_target: Option<String>,
    pub strip_status: bool,
    pub min_keywords: Vec<String>,
    pub notified_pane_ids: HashSet<String>,
}

impl App {
    pub fn new(config: &Config) -> Self {
        let (instances, bell_ids) = state::read_state_files().unwrap_or_default();
        let filtered_indices: Vec<usize> = (0..instances.len()).collect();
        let min_keywords = compute_min_keywords(&instances);

        let current_pane = tmux::current_pane_id();
        let selected = current_pane
            .and_then(|id| {
                filtered_indices
                    .iter()
                    .position(|&idx| instances.get(idx).map(|i| &i.pane_id) == Some(&id))
            })
            .unwrap_or(0);

        let preview = filtered_indices
            .get(selected)
            .and_then(|&idx| instances.get(idx))
            .map(|inst| tmux::capture_pane(&inst.pane_id, 50, config.strip_status))
            .unwrap_or_default();

        Self {
            instances,
            selected,
            filter: String::new(),
            filtered_indices,
            preview,
            should_quit: false,
            jump_target: None,
            strip_status: config.strip_status,
            min_keywords,
            notified_pane_ids: bell_ids,
        }
    }

    pub fn selected_instance(&self) -> Option<&ClaudeInstance> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.instances.get(idx))
    }

    pub fn refresh(&mut self) {
        let (new_instances, bell_ids) = match state::read_state_files() {
            Some(result) => result,
            None => return, // tmux unavailable, keep stale data
        };

        let old_pane_id = self.selected_instance().map(|i| i.pane_id.clone());

        self.instances = new_instances;
        self.min_keywords = compute_min_keywords(&self.instances);
        self.notified_pane_ids = bell_ids;
        self.apply_filter();

        // Try to keep selection on the same pane
        if let Some(ref old_id) = old_pane_id {
            if let Some(pos) = self
                .filtered_indices
                .iter()
                .position(|&idx| self.instances.get(idx).map(|i| &i.pane_id) == Some(old_id))
            {
                self.selected = pos;
            }
        }

        // Clamp selection to valid range
        self.selected = self
            .selected
            .min(self.filtered_indices.len().saturating_sub(1));
    }

    pub fn update_preview(&mut self) {
        if let Some(inst) = self.selected_instance() {
            self.preview = tmux::capture_pane(&inst.pane_id, 50, self.strip_status);
        } else {
            self.preview = String::new();
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.update_preview();
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered_indices.len() {
            self.selected += 1;
            self.update_preview();
        }
    }

    pub fn jump(&mut self) {
        if let Some(inst) = self.selected_instance() {
            let pane_id = inst.pane_id.clone();
            self.notified_pane_ids.remove(&pane_id);
            self.jump_target = Some(pane_id);
            self.should_quit = true;
        }
    }

    pub fn quit(&mut self) {
        self.should_quit = true;
    }

    pub fn add_filter_char(&mut self, c: char) {
        if c == ' ' {
            return;
        }
        self.filter.push(c);
        self.reset_filter_selection();
        if self.filtered_indices.len() == 1 {
            self.jump();
        }
    }

    pub fn delete_filter_char(&mut self) {
        self.filter.pop();
        self.reset_filter_selection();
    }

    fn reset_filter_selection(&mut self) {
        self.apply_filter();
        self.selected = 0;
        self.update_preview();
    }

    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered_indices = (0..self.instances.len()).collect();
            return;
        }
        self.filtered_indices = filter_by(&self.filter, &self.instances, &self.min_keywords);
    }
}

fn filter_by(filter: &str, instances: &[ClaudeInstance], keywords: &[String]) -> Vec<usize> {
    let filter_lower = filter.to_lowercase();

    // Priority 1: Exact keyword match → direct jump
    let exact: Vec<usize> = keywords
        .iter()
        .enumerate()
        .filter(|(_, k)| k.to_lowercase() == filter_lower)
        .map(|(i, _)| i)
        .collect();
    if !exact.is_empty() {
        return exact;
    }

    // Priority 2: Keyword prefix match
    let prefix: Vec<usize> = keywords
        .iter()
        .enumerate()
        .filter(|(_, k)| k.to_lowercase().starts_with(&filter_lower))
        .map(|(i, _)| i)
        .collect();
    if !prefix.is_empty() {
        return prefix;
    }

    // Priority 3: Fuzzy project name match
    instances
        .iter()
        .enumerate()
        .filter(|(_, inst)| fuzzy_match(filter, &inst.project))
        .map(|(i, _)| i)
        .collect()
}

fn fuzzy_match(pattern: &str, target: &str) -> bool {
    let mut target_chars = target.chars().flat_map(|c| c.to_lowercase());
    for pc in pattern.chars().flat_map(|c| c.to_lowercase()) {
        if !target_chars.any(|tc| tc == pc) {
            return false;
        }
    }
    true
}

fn compute_min_keywords(instances: &[ClaudeInstance]) -> Vec<String> {
    // Step 1: Collect unique project names
    let mut unique_projects: Vec<&str> = Vec::new();
    {
        let mut seen = std::collections::HashSet::new();
        for inst in instances {
            if seen.insert(inst.project.as_str()) {
                unique_projects.push(&inst.project);
            }
        }
    }

    // For each unique project, find the shortest prefix that uniquely identifies it
    let mut prefix_map: HashMap<&str, String> = HashMap::new();
    for &proj in &unique_projects {
        let chars: Vec<char> = proj.chars().collect();
        let mut prefix = String::new();
        for len in 1..=chars.len() {
            prefix = chars[..len].iter().collect();
            let lower_prefix = prefix.to_lowercase();
            let match_count = unique_projects
                .iter()
                .filter(|&&other| other.to_lowercase().starts_with(&lower_prefix))
                .count();
            if match_count == 1 {
                break;
            }
        }
        prefix_map.insert(proj, prefix);
    }

    // Step 2: Count occurrences of each project
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for inst in instances {
        *counts.entry(inst.project.as_str()).or_insert(0) += 1;
    }

    // Build keywords: prefix+number for duplicates, prefix only for unique
    let mut seen_counts: HashMap<&str, usize> = HashMap::new();
    instances
        .iter()
        .map(|inst| {
            let prefix = &prefix_map[inst.project.as_str()];
            if counts[inst.project.as_str()] == 1 {
                prefix.clone()
            } else {
                let idx = seen_counts.entry(inst.project.as_str()).or_insert(0);
                *idx += 1;
                format!("{}{}", prefix, idx)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Status;

    fn inst(project: &str) -> ClaudeInstance {
        ClaudeInstance {
            pane_id: String::new(),
            project: project.to_string(),
            status: Status::Idle,
            position: String::new(),
            last_prompt: String::new(),
        }
    }

    fn make_test_data() -> (Vec<ClaudeInstance>, Vec<String>) {
        let instances = vec![
            inst("mono-and.blog"),
            inst("mono-and.blog"),
            inst("dotfiles"),
            inst("himawari"),
            inst("himawari"),
            inst("claude-panes"),
        ];
        let keywords = compute_min_keywords(&instances);
        (instances, keywords)
    }

    fn filter_indices(
        filter: &str,
        instances: &[ClaudeInstance],
        keywords: &[String],
    ) -> Vec<usize> {
        filter_by(filter, instances, keywords)
    }

    // --- min_keywords tests ---

    #[test]
    fn test_min_keywords_unique_projects() {
        let instances = vec![
            inst("mono-and.blog"),
            inst("dotfiles"),
            inst("himawari"),
            inst("claude-panes"),
        ];
        let kw = compute_min_keywords(&instances);
        assert_eq!(kw, vec!["m", "d", "h", "c"]);
    }

    #[test]
    fn test_min_keywords_duplicate_projects() {
        let (_, kw) = make_test_data();
        assert_eq!(kw, vec!["m1", "m2", "d", "h1", "h2", "c"]);
    }

    #[test]
    fn test_min_keywords_shared_prefix() {
        let instances = vec![inst("claude-panes"), inst("claude-test")];
        let kw = compute_min_keywords(&instances);
        assert_eq!(kw, vec!["claude-p", "claude-t"]);
    }

    // --- filter tests: exact keyword jump ---

    #[test]
    fn test_filter_d_jumps_to_dotfiles() {
        let (inst, kw) = make_test_data();
        // "d" exact matches keyword "d" → only dotfiles (index 2)
        assert_eq!(filter_indices("d", &inst, &kw), vec![2]);
    }

    #[test]
    fn test_filter_c_jumps_to_claude_panes() {
        let (inst, kw) = make_test_data();
        assert_eq!(filter_indices("c", &inst, &kw), vec![5]);
    }

    #[test]
    fn test_filter_m2_jumps_to_second_mono() {
        let (inst, kw) = make_test_data();
        assert_eq!(filter_indices("m2", &inst, &kw), vec![1]);
    }

    #[test]
    fn test_filter_h1_jumps_to_first_himawari() {
        let (inst, kw) = make_test_data();
        assert_eq!(filter_indices("h1", &inst, &kw), vec![3]);
    }

    // --- filter tests: keyword prefix (partial) ---

    #[test]
    fn test_filter_m_shows_both_monos() {
        let (inst, kw) = make_test_data();
        // "m" is prefix of "m1" and "m2", also fuzzy matches "mono-and.blog"
        let result = filter_indices("m", &inst, &kw);
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn test_filter_h_shows_both_himawaris() {
        let (inst, kw) = make_test_data();
        let result = filter_indices("h", &inst, &kw);
        assert_eq!(result, vec![3, 4]);
    }

    // --- filter tests: fuzzy project name ---

    #[test]
    fn test_filter_mono_matches_by_project_name() {
        let (inst, kw) = make_test_data();
        let result = filter_indices("mono", &inst, &kw);
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn test_filter_dot_matches_dotfiles() {
        let (inst, kw) = make_test_data();
        let result = filter_indices("dot", &inst, &kw);
        assert_eq!(result, vec![2]);
    }

    // --- App method tests ---

    fn make_app(projects: &[&str]) -> App {
        let instances: Vec<ClaudeInstance> = projects
            .iter()
            .map(|p| ClaudeInstance {
                pane_id: String::new(),
                project: p.to_string(),
                status: Status::Idle,
                position: String::new(),
                last_prompt: String::new(),
            })
            .collect();
        let min_keywords = compute_min_keywords(&instances);
        let filtered_indices = (0..instances.len()).collect();
        App {
            instances,
            selected: 0,
            filter: String::new(),
            filtered_indices,
            preview: String::new(),
            should_quit: false,
            jump_target: None,
            strip_status: true,
            min_keywords,
            notified_pane_ids: HashSet::new(),
        }
    }

    #[test]
    fn move_up_at_top() {
        let mut app = make_app(&["alpha", "beta", "gamma"]);
        app.move_up();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn move_up_decrements() {
        let mut app = make_app(&["alpha", "beta", "gamma"]);
        app.selected = 2;
        app.move_up();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn move_down_at_bottom() {
        let mut app = make_app(&["alpha", "beta", "gamma"]);
        app.selected = 2;
        app.move_down();
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn move_down_increments() {
        let mut app = make_app(&["alpha", "beta", "gamma"]);
        app.move_down();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn quit_sets_flag() {
        let mut app = make_app(&["alpha"]);
        app.quit();
        assert!(app.should_quit);
    }

    #[test]
    fn jump_sets_target() {
        let mut app = make_app(&["alpha"]);
        app.jump();
        assert!(app.jump_target.is_some());
        assert!(app.should_quit);
    }

    #[test]
    fn jump_empty_list() {
        let mut app = make_app(&[]);
        app.jump();
        assert_eq!(app.jump_target, None);
        assert!(!app.should_quit);
    }

    #[test]
    fn add_filter_char_appends() {
        let mut app = make_app(&["alpha", "apex", "beta"]);
        app.add_filter_char('a');
        assert_eq!(app.filter, "a");
    }

    #[test]
    fn add_filter_space_ignored() {
        let mut app = make_app(&["alpha"]);
        app.add_filter_char(' ');
        assert_eq!(app.filter, "");
    }

    #[test]
    fn add_filter_auto_jump() {
        let mut app = make_app(&["alpha", "beta"]);
        // keyword "b" exact-matches beta → 1 result → auto jump
        app.add_filter_char('b');
        assert!(app.should_quit);
        assert!(app.jump_target.is_some());
    }

    #[test]
    fn delete_filter_char_removes() {
        let mut app = make_app(&["alpha", "beta"]);
        app.filter = "ab".to_string();
        app.delete_filter_char();
        assert_eq!(app.filter, "a");
    }

    #[test]
    fn delete_filter_empty_safe() {
        let mut app = make_app(&["alpha"]);
        app.delete_filter_char(); // should not panic
        assert_eq!(app.filter, "");
    }

    #[test]
    fn selected_instance_valid() {
        let app = make_app(&["alpha", "beta"]);
        let inst = app.selected_instance().unwrap();
        assert_eq!(inst.project, "alpha");
    }

    #[test]
    fn selected_instance_empty() {
        let app = make_app(&[]);
        assert!(app.selected_instance().is_none());
    }

    #[test]
    fn filter_resets_selection() {
        let mut app = make_app(&["alpha", "apex", "beta"]);
        app.selected = 2;
        // 'a' prefix-matches keywords "al" and "ap" → 2 results, no auto-jump
        app.add_filter_char('a');
        assert_eq!(app.selected, 0);
    }

    // --- notification tests ---

    fn make_app_with_panes(panes: &[(&str, &str, Status)]) -> App {
        let instances: Vec<ClaudeInstance> = panes
            .iter()
            .map(|(pane_id, project, status)| ClaudeInstance {
                pane_id: pane_id.to_string(),
                project: project.to_string(),
                status: status.clone(),
                position: String::new(),
                last_prompt: String::new(),
            })
            .collect();
        let min_keywords = compute_min_keywords(&instances);
        let filtered_indices = (0..instances.len()).collect();
        App {
            instances,
            selected: 0,
            filter: String::new(),
            filtered_indices,
            preview: String::new(),
            should_quit: false,
            jump_target: None,
            strip_status: true,
            min_keywords,
            notified_pane_ids: HashSet::new(),
        }
    }

    #[test]
    fn jump_clears_notification() {
        let mut app = make_app_with_panes(&[
            ("%0", "proj-a", Status::Waiting),
            ("%1", "proj-b", Status::Waiting),
        ]);
        app.notified_pane_ids.insert("%0".into());
        app.notified_pane_ids.insert("%1".into());
        // Jump to first item (selected=0 → %0)
        app.jump();
        assert!(!app.notified_pane_ids.contains("%0"));
        assert!(app.notified_pane_ids.contains("%1"));
    }
}
