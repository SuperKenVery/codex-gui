use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

const PROJECT_FOLD_DURATION: Duration = Duration::from_millis(190);

#[derive(Clone, Debug)]
pub(super) struct ProjectFoldAnimation {
    pub(super) project_path: String,
    from: f32,
    to: f32,
    pub(super) duration: Duration,
    pub(super) generation: u64,
    progress: Rc<Cell<f32>>,
}

impl ProjectFoldAnimation {
    pub(super) fn new(project_path: String, generation: u64, from: f32, expanding: bool) -> Self {
        let to = if expanding { 1. } else { 0. };
        Self {
            project_path,
            generation,
            from,
            to,
            duration: PROJECT_FOLD_DURATION.mul_f32((to - from).abs()),
            progress: Rc::new(Cell::new(from)),
        }
    }

    pub(super) fn update(&self, delta: f32) {
        self.progress.set(self.from + (self.to - self.from) * delta);
    }

    pub(super) fn current(&self) -> f32 {
        self.progress.get()
    }

    pub(super) fn is_expanding(&self) -> bool {
        self.to == 1.
    }

    pub(super) fn is_collapsing(&self) -> bool {
        !self.is_expanding()
    }
}

pub(super) fn project_child_reveal_progress(
    group_progress: f32,
    child_index: usize,
    child_count: usize,
) -> f32 {
    if child_count == 0 {
        return 1.;
    }
    (group_progress.clamp(0., 1.) * child_count as f32 - child_index as f32).clamp(0., 1.)
}

#[cfg(test)]
mod tests {
    use super::project_child_reveal_progress;

    #[test]
    fn project_children_reveal_from_top_to_bottom() {
        assert_eq!(project_child_reveal_progress(0., 0, 3), 0.);
        assert_eq!(project_child_reveal_progress(0.25, 0, 3), 0.75);
        assert_eq!(project_child_reveal_progress(0.5, 0, 3), 1.);
        assert_eq!(project_child_reveal_progress(0.5, 1, 3), 0.5);
        assert_eq!(project_child_reveal_progress(0.5, 2, 3), 0.);
        assert_eq!(project_child_reveal_progress(1., 2, 3), 1.);
    }
}
