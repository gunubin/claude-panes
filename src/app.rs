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
}

impl App {
    pub fn new(config: &Config) -> Self {
        let instances = state::read_state_files().unwrap_or_default();
        let filtered_indices: Vec<usize> = (0..instances.len()).collect();

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
        }
    }

    pub fn selected_instance(&self) -> Option<&ClaudeInstance> {
        self.filtered_indices
            .get(self.selected)
            .and_then(|&idx| self.instances.get(idx))
    }

    pub fn refresh(&mut self) {
        let new_instances = match state::read_state_files() {
            Some(instances) => instances,
            None => return, // tmux unavailable, keep stale data
        };

        let old_pane_id = self.selected_instance().map(|i| i.pane_id.clone());

        self.instances = new_instances;
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
        if self.filtered_indices.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered_indices.len() {
            self.selected = self.filtered_indices.len() - 1;
        }
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
            self.jump_target = Some(inst.pane_id.clone());
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
        self.apply_filter();
        self.selected = 0;
        self.update_preview();
    }

    pub fn delete_filter_char(&mut self) {
        self.filter.pop();
        self.apply_filter();
        self.selected = 0;
        self.update_preview();
    }

    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.apply_filter();
        self.selected = 0;
        self.update_preview();
    }

    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            self.filtered_indices = (0..self.instances.len()).collect();
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.filtered_indices = self
                .instances
                .iter()
                .enumerate()
                .filter(|(_, inst)| inst.project.to_lowercase().contains(&filter_lower))
                .map(|(i, _)| i)
                .collect();
        }
    }
}
