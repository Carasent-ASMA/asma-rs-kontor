//! Pure selected-layout and branch identity refusals, without filesystem effects.

use kontor_core::branch::{BranchType, ProjectBranchRules, TrackerKey};
use kontor_core::id::TopologyNodeId;
use kontor_core::placement::{ConsultationLayout, managed_root};

fn policy() -> ConsultationLayout {
    ConsultationLayout {
        worktree_layout: "managed/trees/<key-slug>/<module>".into(),
        template: "consultations/<key-slug>/<node>".into(),
        branch_rules: ProjectBranchRules {
            project_key: "EXM".into(),
            types: vec![BranchType::Feat, BranchType::Chore],
            pattern: "^(feat|chore)/<KEY>-<number>-<slug>$".into(),
            commit_pattern: "^<KEY>-<number> <summary>$".into(),
        },
        base_branch: "release/current".into(),
    }
}

#[test]
fn consultations_use_the_exact_managed_prefix_key_and_node_without_a_module() {
    let layout = policy();
    let first = TopologyNodeId::generate();
    let second = TopologyNodeId::generate();
    for (key, node) in [("EXM-17", first), ("EXM-17", second), ("EXM-18", first)] {
        let (path, root, branch) = layout
            .render(&TrackerKey::parse(key).unwrap(), node)
            .unwrap();
        assert_eq!(root, "managed/trees");
        assert_eq!(
            path,
            format!(
                "managed/trees/consultations/{}/{node}",
                key.to_ascii_lowercase()
            )
        );
        assert_eq!(
            branch.to_string(),
            format!("chore/{key}-consultation-{node}")
        );
    }
    assert!(
        layout
            .render(&TrackerKey::parse("OTHER-17").unwrap(), first)
            .is_err()
    );
    let mut disallowed = layout;
    disallowed.branch_rules.types = vec![BranchType::Feat];
    disallowed.branch_rules.pattern = "^(feat)/<KEY>-<number>-<slug>$".into();
    assert!(
        disallowed
            .render(&TrackerKey::parse("EXM-17").unwrap(), first)
            .is_err()
    );
}

#[test]
fn layout_placeholders_are_whole_unique_segments_and_paths_cannot_escape() {
    for template in [
        "",
        "/<key-slug>/<node>",
        "../<key-slug>/<node>",
        "x/./<key-slug>/<node>",
        "x//<key-slug>/<node>",
        "x/<key-slug>/<node>/",
        "x/<key-slug>",
        "x/<node>",
        "x/<key-slug>/<node>/<node>",
        "x/<key-slug>/<key-slug>/<node>",
        "x/<key-slug>/<node>/<module>",
        "x/pre-<key-slug>/<node>",
        "x/<key-slug>/node-<node>",
        "x/{key-slug}/<node>",
        "x/<key-slug>/<node>/*",
        "x/<key-slug>/<node>/ ",
    ] {
        let mut layout = policy();
        layout.template = template.into();
        assert_eq!(
            layout.validate(),
            Err("invalid project.consultation_layout"),
            "{template}"
        );
    }
    for task in [
        "<key-slug>/<module>",
        "x/<key-slug>/<module>/<module>",
        "x/../<module>/<key-slug>",
        "x/<key-slug>/<slot>",
    ] {
        assert!(managed_root(task).is_err());
    }
    assert_eq!(
        managed_root("managed/trees/<module>/<key-slug>"),
        Ok("managed/trees")
    );
    let mut invalid_base = policy();
    invalid_base.base_branch = "../main".into();
    assert_eq!(
        invalid_base.validate(),
        Err("invalid project.forge.base_branch")
    );
}
