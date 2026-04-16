//! UiNode tree, Elle↔Rust conversion, egui rendering, and interaction tracking.

use elle::value::fiber::{SignalBits, SIG_ERROR};
use elle::value::{error_val, TableKey, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

// ── UiNode ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum UiNode {
    // Display
    Label {
        text: String,
    },
    Heading {
        text: String,
    },
    ProgressBar {
        fraction: f64,
        text: Option<String>,
    },
    Separator,
    Spacer {
        size: f32,
    },

    // Input
    Button {
        id: String,
        text: String,
    },
    TextInput {
        id: String,
        hint: Option<String>,
    },
    TextEdit {
        id: String,
        desired_rows: usize,
    },
    Checkbox {
        id: String,
        text: String,
    },
    Slider {
        id: String,
        min: f64,
        max: f64,
    },
    ComboBox {
        id: String,
        options: Vec<String>,
    },

    // Layout
    VLayout {
        children: Vec<UiNode>,
    },
    HLayout {
        children: Vec<UiNode>,
    },
    Centered {
        children: Vec<UiNode>,
    },
    CenteredJustified {
        children: Vec<UiNode>,
    },
    ScrollArea {
        id: String,
        children: Vec<UiNode>,
    },
    Collapsing {
        id: String,
        title: String,
        children: Vec<UiNode>,
    },
    Group {
        children: Vec<UiNode>,
    },
    Grid {
        id: String,
        columns: usize,
        children: Vec<UiNode>,
    },
}

// ── Interactions ─────────────────────────────────────────────────────

#[derive(Default)]
pub struct Interactions {
    pub clicked: HashSet<String>,
    pub text_values: HashMap<String, String>,
    pub check_values: HashMap<String, bool>,
    pub slider_values: HashMap<String, f64>,
    pub combo_values: HashMap<String, String>,
    pub collapsed: HashMap<String, bool>,
    pub closed: bool,
    pub width: f32,
    pub height: f32,
}

// ── Value → UiNode conversion ────────────────────────────────────────

fn get_prop_str(props: &BTreeMap<TableKey, Value>, key: &str) -> Option<String> {
    props
        .get(&TableKey::Keyword(key.into()))
        .and_then(|v| v.with_string(|s| s.to_string()))
}

fn get_prop_keyword(props: &BTreeMap<TableKey, Value>, key: &str) -> Option<String> {
    props
        .get(&TableKey::Keyword(key.into()))
        .and_then(|v| v.as_keyword_name())
}

fn get_prop_id(props: &BTreeMap<TableKey, Value>) -> Option<String> {
    get_prop_keyword(props, "id").or_else(|| get_prop_str(props, "id"))
}

fn get_prop_float(props: &BTreeMap<TableKey, Value>, key: &str) -> Option<f64> {
    props
        .get(&TableKey::Keyword(key.into()))
        .and_then(|v| v.as_float().or_else(|| v.as_int().map(|i| i as f64)))
}

fn get_prop_int(props: &BTreeMap<TableKey, Value>, key: &str) -> Option<i64> {
    props
        .get(&TableKey::Keyword(key.into()))
        .and_then(|v| v.as_int())
}

/// Parse one Elle array `[:tag {props} & args]` into a UiNode.
pub fn value_to_node(val: &Value) -> Result<UiNode, String> {
    let elems = val
        .as_array()
        .ok_or_else(|| format!("ui node must be an array, got {}", val.type_name()))?;

    if elems.is_empty() {
        return Err("ui node array must not be empty".into());
    }

    let tag = elems[0]
        .as_keyword_name()
        .ok_or("first element of ui node must be a keyword")?;

    // Check if second element is a props struct. `as_struct()` returns
    // a sorted slice of (key, value) pairs; collect into a BTreeMap so
    // the existing `get_prop_*` helpers (typed for BTreeMap) work.
    let (props, rest_start) = if elems.len() > 1 {
        if let Some(s) = elems[1].as_struct() {
            let map: BTreeMap<TableKey, Value> = s.iter().cloned().collect();
            (map, 2)
        } else {
            (BTreeMap::new(), 1)
        }
    } else {
        (BTreeMap::new(), 1)
    };

    let rest = &elems[rest_start..];

    match tag.as_str() {
        // ── Display ──
        "label" => {
            let text = rest
                .first()
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_default();
            Ok(UiNode::Label { text })
        }
        "heading" => {
            let text = rest
                .first()
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_default();
            Ok(UiNode::Heading { text })
        }
        "progress-bar" => {
            let fraction = get_prop_float(&props, "fraction").unwrap_or(0.0);
            let text = get_prop_str(&props, "text");
            Ok(UiNode::ProgressBar { fraction, text })
        }
        "separator" => Ok(UiNode::Separator),
        "spacer" => {
            let size = get_prop_float(&props, "size").unwrap_or(8.0) as f32;
            Ok(UiNode::Spacer { size })
        }

        // ── Input ──
        "button" => {
            let id = get_prop_id(&props).ok_or("button requires :id")?;
            let text = rest
                .first()
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_default();
            Ok(UiNode::Button { id, text })
        }
        "text-input" => {
            let id = get_prop_id(&props).ok_or("text-input requires :id")?;
            let hint = get_prop_str(&props, "hint");
            Ok(UiNode::TextInput { id, hint })
        }
        "text-edit" => {
            let id = get_prop_id(&props).ok_or("text-edit requires :id")?;
            let desired_rows = get_prop_int(&props, "rows").unwrap_or(4) as usize;
            Ok(UiNode::TextEdit { id, desired_rows })
        }
        "checkbox" => {
            let id = get_prop_id(&props).ok_or("checkbox requires :id")?;
            let text = rest
                .first()
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_default();
            Ok(UiNode::Checkbox { id, text })
        }
        "slider" => {
            let id = get_prop_id(&props).ok_or("slider requires :id")?;
            let min = get_prop_float(&props, "min").unwrap_or(0.0);
            let max = get_prop_float(&props, "max").unwrap_or(100.0);
            Ok(UiNode::Slider { id, min, max })
        }
        "combo-box" => {
            let id = get_prop_id(&props).ok_or("combo-box requires :id")?;
            let options: Vec<String> = rest
                .first()
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.with_string(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            Ok(UiNode::ComboBox { id, options })
        }

        // ── Layout ──
        "v-layout" => {
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::VLayout {
                children: children?,
            })
        }
        "h-layout" => {
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::HLayout {
                children: children?,
            })
        }
        "centered" => {
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::Centered {
                children: children?,
            })
        }
        "centered-justified" => {
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::CenteredJustified {
                children: children?,
            })
        }
        "scroll-area" => {
            let id = get_prop_id(&props).ok_or("scroll-area requires :id")?;
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::ScrollArea {
                id,
                children: children?,
            })
        }
        "collapsing" => {
            let id = get_prop_id(&props).ok_or("collapsing requires :id")?;
            let title = rest
                .first()
                .and_then(|v| v.with_string(|s| s.to_string()))
                .unwrap_or_default();
            let children: Result<Vec<_>, _> = rest.iter().skip(1).map(value_to_node).collect();
            Ok(UiNode::Collapsing {
                id,
                title,
                children: children?,
            })
        }
        "group" => {
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::Group {
                children: children?,
            })
        }
        "grid" => {
            let id = get_prop_id(&props).ok_or("grid requires :id")?;
            let columns = get_prop_int(&props, "columns").unwrap_or(2) as usize;
            let children: Result<Vec<_>, _> = rest.iter().map(value_to_node).collect();
            Ok(UiNode::Grid {
                id,
                columns,
                children: children?,
            })
        }

        other => Err(format!("unknown ui node type: :{}", other)),
    }
}

/// Parse a top-level Elle tree into a Vec<UiNode>.
pub fn value_to_tree(val: &Value) -> Result<Vec<UiNode>, (SignalBits, Value)> {
    // The tree is either a single node (array starting with keyword)
    // or a list of nodes.
    if let Some(elems) = val.as_array() {
        if !elems.is_empty() && elems[0].is_keyword() {
            // Single node
            let node = value_to_node(val).map_err(|e| (SIG_ERROR, error_val("ui-error", e)))?;
            return Ok(vec![node]);
        }
        // Array of nodes
        elems
            .iter()
            .map(|v| value_to_node(v).map_err(|e| (SIG_ERROR, error_val("ui-error", e))))
            .collect()
    } else {
        Err((
            SIG_ERROR,
            error_val("type-error", "egui/frame: tree must be an array"),
        ))
    }
}

// ── Rendering ────────────────────────────────────────────────────────

pub struct WidgetState {
    pub text_buffers: HashMap<String, String>,
    pub check_states: HashMap<String, bool>,
    pub slider_states: HashMap<String, f64>,
    pub combo_states: HashMap<String, String>,
    pub collapsed_states: HashMap<String, bool>,
}

impl WidgetState {
    pub fn new() -> Self {
        Self {
            text_buffers: HashMap::new(),
            check_states: HashMap::new(),
            slider_states: HashMap::new(),
            combo_states: HashMap::new(),
            collapsed_states: HashMap::new(),
        }
    }
}

pub fn render_tree(
    ui: &mut egui::Ui,
    nodes: &[UiNode],
    state: &mut WidgetState,
    ix: &mut Interactions,
) {
    for node in nodes {
        render_node(ui, node, state, ix);
    }
}

fn render_node(ui: &mut egui::Ui, node: &UiNode, state: &mut WidgetState, ix: &mut Interactions) {
    match node {
        UiNode::Label { text } => {
            ui.label(text.as_str());
        }
        UiNode::Heading { text } => {
            ui.heading(text.as_str());
        }
        UiNode::ProgressBar { fraction, text } => {
            let mut bar = egui::ProgressBar::new(*fraction as f32);
            if let Some(t) = text {
                bar = bar.text(t.as_str());
            }
            ui.add(bar);
        }
        UiNode::Separator => {
            ui.separator();
        }
        UiNode::Spacer { size } => {
            ui.add_space(*size);
        }
        UiNode::Button { id, text } => {
            if ui.button(text.as_str()).clicked() {
                ix.clicked.insert(id.clone());
            }
        }
        UiNode::TextInput { id, hint } => {
            let buf = state.text_buffers.entry(id.clone()).or_default();
            let mut edit = egui::TextEdit::singleline(buf);
            if let Some(h) = hint {
                edit = edit.hint_text(h.as_str());
            }
            ui.add(edit);
            ix.text_values.insert(id.clone(), buf.clone());
        }
        UiNode::TextEdit { id, desired_rows } => {
            let buf = state.text_buffers.entry(id.clone()).or_default();
            ui.add(egui::TextEdit::multiline(buf).desired_rows(*desired_rows));
            ix.text_values.insert(id.clone(), buf.clone());
        }
        UiNode::Checkbox { id, text } => {
            let checked = state.check_states.entry(id.clone()).or_insert(false);
            ui.checkbox(checked, text.as_str());
            ix.check_values.insert(id.clone(), *checked);
        }
        UiNode::Slider { id, min, max } => {
            let val = state.slider_states.entry(id.clone()).or_insert(*min);
            ui.add(egui::Slider::new(val, *min..=*max));
            ix.slider_values.insert(id.clone(), *val);
        }
        UiNode::ComboBox { id, options } => {
            let selected = state
                .combo_states
                .entry(id.clone())
                .or_insert_with(|| options.first().cloned().unwrap_or_default());
            egui::ComboBox::from_id_salt(id.as_str())
                .selected_text(selected.as_str())
                .show_ui(ui, |ui| {
                    for opt in options {
                        ui.selectable_value(selected, opt.clone(), opt.as_str());
                    }
                });
            ix.combo_values.insert(id.clone(), selected.clone());
        }
        UiNode::VLayout { children } => {
            ui.vertical(|ui| render_tree(ui, children, state, ix));
        }
        UiNode::HLayout { children } => {
            ui.horizontal(|ui| render_tree(ui, children, state, ix));
        }
        UiNode::Centered { children } => {
            ui.vertical_centered(|ui| render_tree(ui, children, state, ix));
        }
        UiNode::CenteredJustified { children } => {
            ui.vertical_centered_justified(|ui| render_tree(ui, children, state, ix));
        }
        UiNode::ScrollArea { id, children } => {
            egui::ScrollArea::vertical()
                .id_salt(id.as_str())
                .show(ui, |ui| render_tree(ui, children, state, ix));
        }
        UiNode::Collapsing {
            id,
            title,
            children,
        } => {
            let default_open = *state.collapsed_states.entry(id.clone()).or_insert(true);
            let resp = egui::CollapsingHeader::new(title.as_str())
                .id_salt(id.as_str())
                .default_open(default_open)
                .show(ui, |ui| render_tree(ui, children, state, ix));
            let is_open = resp.fully_open();
            state.collapsed_states.insert(id.clone(), is_open);
            ix.collapsed.insert(id.clone(), !is_open);
        }
        UiNode::Group { children } => {
            ui.group(|ui| render_tree(ui, children, state, ix));
        }
        UiNode::Grid {
            id,
            columns,
            children,
        } => {
            egui::Grid::new(id.as_str())
                .num_columns(*columns)
                .show(ui, |ui| {
                    for (i, child) in children.iter().enumerate() {
                        render_node(ui, child, state, ix);
                        if (i + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        }
    }
}

// ── Interactions → Value ─────────────────────────────────────────────

pub fn interactions_to_value(ix: &Interactions) -> Value {
    let mut fields = BTreeMap::new();

    // :clicks — set of clicked button ids
    let clicks: BTreeSet<Value> = ix.clicked.iter().map(|s| Value::keyword(s)).collect();
    fields.insert(TableKey::Keyword("clicks".into()), Value::set(clicks));

    // :text — struct of text values
    let text_fields: BTreeMap<TableKey, Value> = ix
        .text_values
        .iter()
        .map(|(k, v)| (TableKey::Keyword(k.clone()), Value::string(v.as_str())))
        .collect();
    fields.insert(
        TableKey::Keyword("text".into()),
        Value::struct_from(text_fields),
    );

    // :checks — struct of check states
    let check_fields: BTreeMap<TableKey, Value> = ix
        .check_values
        .iter()
        .map(|(k, v)| (TableKey::Keyword(k.clone()), Value::bool(*v)))
        .collect();
    fields.insert(
        TableKey::Keyword("checks".into()),
        Value::struct_from(check_fields),
    );

    // :sliders — struct of slider values
    let slider_fields: BTreeMap<TableKey, Value> = ix
        .slider_values
        .iter()
        .map(|(k, v)| (TableKey::Keyword(k.clone()), Value::float(*v)))
        .collect();
    fields.insert(
        TableKey::Keyword("sliders".into()),
        Value::struct_from(slider_fields),
    );

    // :combos — struct of combo selections
    let combo_fields: BTreeMap<TableKey, Value> = ix
        .combo_values
        .iter()
        .map(|(k, v)| (TableKey::Keyword(k.clone()), Value::string(v.as_str())))
        .collect();
    fields.insert(
        TableKey::Keyword("combos".into()),
        Value::struct_from(combo_fields),
    );

    // :collapsed — struct of collapsed states
    let collapsed_fields: BTreeMap<TableKey, Value> = ix
        .collapsed
        .iter()
        .map(|(k, v)| (TableKey::Keyword(k.clone()), Value::bool(*v)))
        .collect();
    fields.insert(
        TableKey::Keyword("collapsed".into()),
        Value::struct_from(collapsed_fields),
    );

    // :closed
    fields.insert(TableKey::Keyword("closed".into()), Value::bool(ix.closed));

    // :size [w h]
    fields.insert(
        TableKey::Keyword("size".into()),
        Value::array(vec![
            Value::int(ix.width as i64),
            Value::int(ix.height as i64),
        ]),
    );

    Value::struct_from(fields)
}
