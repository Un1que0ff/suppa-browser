// Ported from ..\..\src\groups.rs (Windows). Keep the two in step.
// Tab groups (like Chrome):
//  * Drag a tab and hold it over the middle of another tab -> both form a
//    group (or the tab joins the other tab's group).
//  * A group has a coloured label in front of its tabs. Click = collapse /
//    expand; right-click = rename, colour, ungroup, close group.
//  * Tabs of a group always stay next to each other.
//  * Saved with the session ('#' lines in session.txt).
use serde_json::json;
use crate::popup::POINT;

use crate::app::{self, Group, State};
use crate::popup::{self, Item};
use crate::{tabs, ui};

/// Group colours (name, colour).
pub const COLORS: [(&str, &str); 8] = [
    ("Purple", "#b07cff"),
    ("Green", "#4cd37b"),
    ("Cyan", "#3cd6e0"),
    ("Yellow", "#ffd43b"),
    ("Red", "#ff8a8a"),
    ("Blue", "#5b9dff"),
    ("Orange", "#ff9a3c"),
    ("Pink", "#ff6fb5"),
];

fn new_group(s: &mut State) -> u32 {
    let id = s.next_group;
    s.next_group += 1;
    let used: Vec<&str> = s.groups.iter().map(|g| g.color.as_str()).collect();
    let color = COLORS.iter().map(|c| c.1).find(|c| !used.contains(c)).unwrap_or(COLORS[(id as usize - 1) % COLORS.len()].1);
    s.groups.push(Group { id, name: String::new(), color: color.to_string(), collapsed: false });
    id
}

/// Removes groups without tabs.
pub fn cleanup(s: &mut State) {
    let used: Vec<u32> = s.tabs.iter().map(|t| t.group).filter(|&g| g != 0).collect();
    s.groups.retain(|g| used.contains(&g.id));
}

/// Index of the active tab after tabs were moved around.
fn keep_active(s: &mut State, active_id: u32) {
    if let Some(i) = s.tabs.iter().position(|t| t.id == active_id) {
        s.active = i;
    }
}

/// Dropped tab `from` on the middle of tab `target`: group them.
pub fn group_with(from: usize, target: usize) {
    let created = app::with(|s| {
        if from >= s.tabs.len() || target >= s.tabs.len() || from == target {
            return None;
        }
        let active_id = s.tabs[s.active].id;
        let mut created = None;
        let gid = match s.tabs[target].group {
            0 => {
                let id = new_group(s);
                s.tabs[target].group = id;
                created = Some(id);
                id
            }
            g => g,
        };
        let mut t = s.tabs.remove(from);
        t.group = gid;
        let last = s.tabs.iter().rposition(|x| x.group == gid).unwrap_or(0);
        s.tabs.insert(last + 1, t);
        keep_active(s, active_id);
        expand(s, gid);
        cleanup(s);
        ui::request_sync();
        created
    })
    .flatten();
    if let Some(id) = created {
        start_rename(id);
    }
}

/// After a tab was dragged to a new place: it joins the group it was dropped
/// into, or leaves its group if dropped outside it.
pub fn after_move(s: &mut State, idx: usize) {
    if idx >= s.tabs.len() {
        return;
    }
    let left = if idx > 0 { s.tabs[idx - 1].group } else { 0 };
    let right = s.tabs.get(idx + 1).map(|t| t.group).unwrap_or(0);
    let own = s.tabs[idx].group;
    s.tabs[idx].group = if left != 0 && left == right {
        left
    } else if own != 0 && (own == left || own == right) {
        own
    } else {
        0
    };
    cleanup(s);
}

/// A new tab opened from a grouped tab (link, popup) joins that group.
pub fn inherit(s: &mut State, idx: usize) {
    let opener = s.tabs.get(s.active).map(|t| t.group).unwrap_or(0);
    let next = s.tabs.get(idx + 1).map(|t| t.group).unwrap_or(0);
    if opener != 0 && (next == opener || s.active + 1 == idx) {
        if let Some(t) = s.tabs.get_mut(idx) {
            t.group = opener;
        }
    }
}

fn expand(s: &mut State, gid: u32) {
    if let Some(g) = s.groups.iter_mut().find(|g| g.id == gid) {
        g.collapsed = false;
    }
}

/// Tab right-click -> Add to new group.
pub fn add_to_new_group(idx: usize) {
    let id = app::with(|s| {
        if idx >= s.tabs.len() {
            return None;
        }
        let active_id = s.tabs[s.active].id;
        let id = new_group(s);
        // Out of any group it was in: put it after that group.
        let old = s.tabs[idx].group;
        let mut t = s.tabs.remove(idx);
        t.group = id;
        let at = if old != 0 { s.tabs.iter().rposition(|x| x.group == old).map(|p| p + 1).unwrap_or(idx) } else { idx };
        s.tabs.insert(at.min(s.tabs.len()), t);
        keep_active(s, active_id);
        cleanup(s);
        ui::request_sync();
        Some(id)
    })
    .flatten();
    if let Some(id) = id {
        start_rename(id);
    }
}

/// Tab right-click -> Remove from group (moves it just after the group).
pub fn remove_from_group(idx: usize) {
    app::with(|s| {
        let Some(old) = s.tabs.get(idx).map(|t| t.group).filter(|&g| g != 0) else { return };
        let active_id = s.tabs[s.active].id;
        let mut t = s.tabs.remove(idx);
        t.group = 0;
        let at = s.tabs.iter().rposition(|x| x.group == old).map(|p| p + 1).unwrap_or(idx);
        s.tabs.insert(at.min(s.tabs.len()), t);
        keep_active(s, active_id);
        cleanup(s);
        ui::request_sync();
    });
}

/// Click on a group label: collapse / expand. The selected tab can't be hidden,
/// so collapsing switches to the next tab outside the group.
pub fn toggle(gid: u32) {
    let switch_to = app::with(|s| {
        let collapsed = s.groups.iter().find(|g| g.id == gid).map(|g| g.collapsed)?;
        let mut switch_to = None;
        if !collapsed && s.tabs.get(s.active).map(|t| t.group) == Some(gid) {
            let after = s.tabs.iter().enumerate().skip(s.active).find(|(_, t)| t.group != gid).map(|(i, _)| i);
            let before = s.tabs.iter().enumerate().take(s.active).rev().find(|(_, t)| t.group != gid).map(|(i, _)| i);
            switch_to = Some(after.or(before)?);
        }
        if let Some(g) = s.groups.iter_mut().find(|g| g.id == gid) {
            g.collapsed = !collapsed;
        }
        ui::request_sync();
        Some(switch_to)
    })
    .flatten()
    .flatten();
    if let Some(i) = switch_to {
        tabs::activate(i);
    }
}

/// Tab right-click -> Ungroup: dissolves the group this tab is in.
pub fn ungroup_tab(idx: usize) {
    if let Some(Some(g)) = app::with(|s| s.tabs.get(idx).map(|t| t.group).filter(|&g| g != 0)) {
        ungroup(g);
    }
}

fn ungroup(gid: u32) {
    app::with(|s| {
        for t in s.tabs.iter_mut().filter(|t| t.group == gid) {
            t.group = 0;
        }
        cleanup(s);
        ui::request_sync();
    });
}

fn close_group(gid: u32) {
    loop {
        let next = app::with(|s| {
            if s.tabs.iter().all(|t| t.group == gid) && s.tabs.iter().filter(|t| t.group == gid).count() <= 1 {
                return None; // never close the last tab of the window this way
            }
            s.tabs.iter().position(|t| t.group == gid)
        })
        .flatten();
        match next {
            Some(i) => tabs::close_tab(i),
            None => break,
        }
    }
}

fn set_color(gid: u32, color: &str) {
    app::with(|s| {
        if let Some(g) = s.groups.iter_mut().find(|g| g.id == gid) {
            g.color = color.to_string();
        }
        ui::request_sync();
    });
}

pub fn rename(gid: u32, name: &str) {
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(40).collect();
    app::with(|s| {
        if let Some(g) = s.groups.iter_mut().find(|g| g.id == gid) {
            g.name = name;
        }
        ui::request_sync();
    });
}

fn start_rename(gid: u32) {
    ui::focus_bar();
    ui::post(json!({ "type": "group_rename", "id": gid }).to_string());
}

/// Right-click on a group label.
pub fn menu(gid: u32, anchor: POINT) {
    let Some(Some((name, color, collapsed, n))) = app::with(|s| {
        let g = s.groups.iter().find(|g| g.id == gid)?;
        Some((g.name.clone(), g.color.clone(), g.collapsed, s.tabs.iter().filter(|t| t.group == gid).count()))
    }) else {
        return;
    };
    let title = if name.is_empty() { format!("Group ({n} tabs)") } else { format!("{name} ({n} tabs)") };
    let colors: Vec<Item> =
        COLORS.iter().enumerate().map(|(i, (cname, c))| Item::new(100 + i as i64, *cname).checked(color == *c)).collect();
    let items = vec![
        Item::new(0, title).enabled(false),
        Item::sep(),
        Item::new(1, "Rename"),
        Item::sub("Colour", colors),
        Item::new(2, if collapsed { "Expand" } else { "Collapse" }),
        Item::sep(),
        Item::new(3, "Ungroup"),
        Item::new(4, "Close group"),
    ];
    popup::show("group", items, anchor, false, move |pick| match pick {
        Some(1) => start_rename(gid),
        Some(2) => toggle(gid),
        Some(3) => ungroup(gid),
        Some(4) => close_group(gid),
        Some(c) if (100..108).contains(&c) => set_color(gid, COLORS[(c - 100) as usize].1),
        _ => {}
    });
}

/// Groups for the bar's sync message.
pub fn json(s: &State) -> serde_json::Value {
    serde_json::Value::Array(
        s.groups
            .iter()
            .map(|g| json!({ "id": g.id, "name": g.name, "color": g.color, "collapsed": g.collapsed }))
            .collect(),
    )
}

// ---- Session ---------------------------------------------------------------------------

/// '#group' lines for session.txt (the '#tabgroup' lines are added per tab).
pub fn session_meta_groups(s: &State) -> Vec<String> {
    s.groups
        .iter()
        .map(|g| format!("#group\t{}\t{}\t{}\t{}", g.id, g.color, g.collapsed as u8, g.name.replace(['\t', '\n'], " ")))
        .collect()
}

/// Restores groups from session.txt after the tabs were added (same order).
pub fn restore(meta: &[String]) {
    app::with(|s| {
        for line in meta {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.as_slice() {
                ["#group", id, color, collapsed, rest @ ..] => {
                    if let Ok(id) = id.parse::<u32>() {
                        s.groups.push(Group {
                            id,
                            name: rest.join(" "),
                            color: color.to_string(),
                            collapsed: *collapsed == "1",
                        });
                        s.next_group = s.next_group.max(id + 1);
                    }
                }
                ["#tabgroup", idx, gid] => {
                    if let (Ok(i), Ok(g)) = (idx.parse::<usize>(), gid.parse::<u32>()) {
                        if let Some(t) = s.tabs.get_mut(i) {
                            t.group = g;
                        }
                    }
                }
                _ => {}
            }
        }
        cleanup(s);
        // The selected tab is never inside a collapsed group.
        let active_group = s.tabs.get(s.active).map(|t| t.group).unwrap_or(0);
        expand(s, active_group);
    });
}
