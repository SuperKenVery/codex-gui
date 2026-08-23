use std::collections::HashSet;

/// A flattened row in the sidebar list.
#[derive(Clone, Debug)]
pub enum SidebarRow<Project, Chat> {
    /// "Projects" or "Chats" label
    SectionHeader { label: &'static str },
    /// Project folder that contains chats in it
    Project {
        project_index: usize,
        project: Project,
        selected: bool,
        expanded: bool,
    },
    /// A thread with a user-specified project dir
    Chat {
        project_index: usize,
        chat_index: usize,
        chat: Chat,
        selected: bool,
    },
    /// A project-less chat
    ProjectlessChat {
        chat_index: usize,
        chat: Chat,
        selected: bool,
    },
    /// Show more/show all button
    ShowMore { kind: PaginateKind },
}

#[derive(Clone, Debug)]
pub enum PaginateKind {
    Projects,
    ProjectlessChats,
    ProjectChats { path: String },
}

pub fn build_sidebar_rows<Project: Clone, Chat: Clone>(
    projects: &[Project],
    visible_project_count: usize,
    projectless_chats: &[Chat],
    visible_projectless_count: usize,
    active_project: usize,
    active_chat: usize,
    active_projectless_chat: Option<usize>,
    active_project_path: Option<&str>,
    active_project_chats: &[Chat],
    visible_project_chat_count: usize,
    collapsed_projects: &HashSet<String>,
) -> Vec<SidebarRow<Project, Chat>> {
    let mut rows = Vec::with_capacity(projects.len() + projectless_chats.len() + 6);

    if !projects.is_empty() {
        rows.push(SidebarRow::SectionHeader { label: "Projects" });
        for (project_index, project) in projects.iter().take(visible_project_count).enumerate() {
            let selected = project_index == active_project && active_projectless_chat.is_none();
            let expanded = selected
                && active_project_path.is_some_and(|path| !collapsed_projects.contains(path));
            rows.push(SidebarRow::Project {
                project_index,
                project: project.clone(),
                selected,
                expanded,
            });

            if !expanded {
                continue;
            }

            let path = active_project_path.expect("an expanded project always has a path");
            rows.extend(
                active_project_chats
                    .iter()
                    .take(visible_project_chat_count)
                    .enumerate()
                    .map(|(chat_index, chat)| SidebarRow::Chat {
                        project_index,
                        chat_index,
                        chat: chat.clone(),
                        selected: chat_index == active_chat,
                    }),
            );
            if active_project_chats.len() > visible_project_chat_count {
                rows.push(SidebarRow::ShowMore {
                    kind: PaginateKind::ProjectChats {
                        path: path.to_owned(),
                    },
                });
            }
        }
        if projects.len() > visible_project_count {
            rows.push(SidebarRow::ShowMore {
                kind: PaginateKind::Projects,
            });
        }
    }

    if !projectless_chats.is_empty() {
        rows.push(SidebarRow::SectionHeader { label: "Chats" });
        rows.extend(
            projectless_chats
                .iter()
                .take(visible_projectless_count)
                .enumerate()
                .map(|(chat_index, chat)| SidebarRow::ProjectlessChat {
                    chat_index,
                    chat: chat.clone(),
                    selected: active_projectless_chat == Some(chat_index),
                }),
        );
        if projectless_chats.len() > visible_projectless_count {
            rows.push(SidebarRow::ShowMore {
                kind: PaginateKind::ProjectlessChats,
            });
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_only_the_active_projects_visible_threads() {
        let projects = vec!["one", "two", "three"];
        let chats = (0..8).collect::<Vec<_>>();
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &[] as &[i32],
            0,
            1,
            2,
            None,
            Some("two"),
            &chats,
            5,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 10); // header + 3 projects + 5 chats + show-more.
        assert!(matches!(rows[0], SidebarRow::SectionHeader { .. }));
        assert!(matches!(
            rows[2],
            SidebarRow::Project {
                project_index: 1,
                expanded: true,
                ..
            }
        ));
        assert!(matches!(
            rows[5],
            SidebarRow::Chat {
                chat_index: 2,
                selected: true,
                ..
            }
        ));
        assert!(matches!(rows[8], SidebarRow::ShowMore { .. }));
    }

    #[test]
    fn projectless_chats_render_in_their_own_section() {
        let projects = vec!["one"];
        let projectless = vec![10, 11, 12];
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &projectless,
            projectless.len(),
            0,
            0,
            Some(1),
            Some("one"),
            &[] as &[i32],
            0,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 5); // projects header + project + chats header + 3 chats.
        assert!(matches!(rows[0], SidebarRow::SectionHeader { .. }));
        assert!(matches!(
            rows[1],
            SidebarRow::Project {
                selected: false,
                expanded: false,
                ..
            }
        ));
        assert!(matches!(rows[2], SidebarRow::SectionHeader { .. }));
        assert!(matches!(
            rows[3],
            SidebarRow::ProjectlessChat {
                chat_index: 0,
                selected: false,
                ..
            }
        ));
        assert!(matches!(
            rows[4],
            SidebarRow::ProjectlessChat {
                chat_index: 2,
                selected: true,
                ..
            }
        ));
    }

    #[test]
    fn pagination_adds_show_more_rows_for_both_sections() {
        let projects = (0..15).collect::<Vec<_>>();
        let projectless = (0..15).collect::<Vec<_>>();
        let rows = build_sidebar_rows(
            &projects,
            10,
            &projectless,
            10,
            0,
            0,
            None,
            Some("0"),
            &[] as &[i32],
            0,
            &HashSet::new(),
        );

        assert_eq!(rows.len(), 24); // header + 10 projects + show-more + header + 10 chats + show-more.
        let kinds = rows
            .iter()
            .filter_map(|row| match row {
                SidebarRow::ShowMore { kind } => Some(kind),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(kinds.len(), 2);
        assert!(matches!(kinds[0], PaginateKind::Projects));
        assert!(matches!(kinds[1], PaginateKind::ProjectlessChats));
    }

    #[test]
    fn expanded_show_all_and_collapse_change_only_the_flattened_rows() {
        let projects = vec!["one", "two"];
        let chats = (0..8).collect::<Vec<_>>();
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &[] as &[i32],
            0,
            0,
            0,
            None,
            Some("one"),
            &chats,
            chats.len(),
            &HashSet::new(),
        );
        assert_eq!(rows.len(), 11); // header + 2 projects + all 8 chats.
        assert!(
            !rows
                .iter()
                .any(|row| matches!(row, SidebarRow::ShowMore { .. }))
        );

        let collapsed = HashSet::from(["one".to_string()]);
        let rows = build_sidebar_rows(
            &projects,
            projects.len(),
            &[] as &[i32],
            0,
            0,
            0,
            None,
            Some("one"),
            &chats,
            chats.len(),
            &collapsed,
        );
        assert_eq!(rows.len(), 3); // header + 2 project rows.
        assert!(rows.iter().all(|row| matches!(
            row,
            SidebarRow::Project { .. } | SidebarRow::SectionHeader { .. }
        )));
    }
}
