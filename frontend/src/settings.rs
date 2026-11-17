use crate::rest::{BuildCommands, CustomCommand};

use gloo_net::http::Request;
use kaja_web::prelude::*;
use std::collections::HashMap;

use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlInputElement};

#[component("project-settings")]
struct Settings {
    #[attribute]
    pub project: String,
    #[attribute]
    pub git: String,
    #[attribute]
    pub clone: String,
    #[attribute]
    pub build: String,
    #[attribute]
    pub test: String,
    #[attribute]
    pub release: String,
    #[attribute]
    pub clean: String,
    #[attribute]
    pub terminal: String,
    #[attribute]
    pub custom: Vec<CustomCommand>,
    #[attribute]
    pub tasks: Vec<String>,
    #[attribute]
    pub order: HashMap<String, i32>,
}

impl Component for SettingsComponent {
    fn connected(&self) {
        let id = self.get_id();
        log!("Connected:", &id);
        self.render();
    }

    fn disconnected(&self) {
        log!("Disconnected:", self.get_id());
    }

    fn attribute_changed(&self, _name: &str, _old_value: &str, _new_value: &str) {}
}

impl SettingsComponent {
    fn render(&self) {
        let id = self.get_id();
        let element = match get_element_by_id(&id) {
            Some(element) => element,
            None => {
                log!("No root element for build component");
                return;
            }
        };

        let project: String = self.get_attribute("project").unwrap_or_default();
        let git: String = self.get_attribute("git").unwrap_or_default();
        let clone: String = self.get_attribute("clone").unwrap_or_default();
        let build: String = self.get_attribute("build").unwrap_or_default();
        let test: String = self.get_attribute("test").unwrap_or_default();
        let release: String = self.get_attribute("release").unwrap_or_default();
        let clean: String = self.get_attribute("clean").unwrap_or_default();
        let terminal: String = self.get_attribute("terminal").unwrap_or_default();
        let custom: Vec<CustomCommand> = self.get_attribute("custom").unwrap_or_default();
        let tasks: Vec<String> = self.get_attribute("tasks").unwrap_or_default();
        let order: HashMap<String, i32> = self.get_attribute("order").unwrap_or_default();

        let task_order = |task_key: &str, fallback_index: usize| -> i32 {
            if let Some(value) = order.get(task_key) {
                return *value;
            }

            if let Some(position) = tasks.iter().position(|task| task == task_key) {
                return (position as i32) + 1;
            }

            (fallback_index as i32) + 1
        };

        let has_explicit_order = |task_key: &str| -> bool { order.contains_key(task_key) };

        let git_order = task_order("git", 0);
        let clone_order = task_order("clone", 1);
        let build_order = task_order("build", 2);
        let test_order = task_order("test", 3);
        let release_order = task_order("release", 4);
        let clean_order = task_order("clean", 5);
        let terminal_order = task_order("terminal", 6);

        let git_has_explicit_order = has_explicit_order("git");
        let clone_has_explicit_order = has_explicit_order("clone");
        let build_has_explicit_order = has_explicit_order("build");
        let test_has_explicit_order = has_explicit_order("test");
        let release_has_explicit_order = has_explicit_order("release");
        let clean_has_explicit_order = has_explicit_order("clean");
        let terminal_has_explicit_order = has_explicit_order("terminal");

        let content = html! {{
            <p>$(project)</p>
            <p id="save-status"></p>

            <input type="hidden" name="project" value="$(project)" />

            <div id="task-order-list" class="task-order-list">
            <div id="row-git" class="task-order-row" style="order: $(git_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-git" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-git">Git fetch command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="git" name="git" type="text" value="$(git)" />
                        <input id="order-git" class="task-order-input" type="number" value="$(git_order)" oninput="setTaskOrder('$id', 'git')" data-order="$(git_order)" data-explicit-order="$(git_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'git')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'git')">Down</button>
                    </div>
                </div>
            </div>

            <div id="row-clone" class="task-order-row" style="order: $(clone_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-clone" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-clone">Clone command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="clone" name="clone" type="text" value="$(clone)" />
                        <input id="order-clone" class="task-order-input" type="number" value="$(clone_order)" oninput="setTaskOrder('$id', 'clone')" data-order="$(clone_order)" data-explicit-order="$(clone_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'clone')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'clone')">Down</button>
                    </div>
                </div>
            </div>

            <div id="row-build" class="task-order-row" style="order: $(build_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-build" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-build">Build command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="build" name="build" type="text" value="$(build)" />
                        <input id="order-build" class="task-order-input" type="number" value="$(build_order)" oninput="setTaskOrder('$id', 'build')" data-order="$(build_order)" data-explicit-order="$(build_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'build')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'build')">Down</button>
                    </div>
                </div>
            </div>

            <div id="row-test" class="task-order-row" style="order: $(test_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-test" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-test">Test command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="test" name="test" type="text" value="$(test)" />
                        <input id="order-test" class="task-order-input" type="number" value="$(test_order)" oninput="setTaskOrder('$id', 'test')" data-order="$(test_order)" data-explicit-order="$(test_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'test')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'test')">Down</button>
                    </div>
                </div>
            </div>

            <div id="row-release" class="task-order-row" style="order: $(release_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-release" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-release">Release command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="release" name="release" type="text" value="$(release)" />
                        <input id="order-release" class="task-order-input" type="number" value="$(release_order)" oninput="setTaskOrder('$id', 'release')" data-order="$(release_order)" data-explicit-order="$(release_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'release')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'release')">Down</button>
                    </div>
                </div>
            </div>

            <div id="row-clean" class="task-order-row" style="order: $(clean_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-clean" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-clean">Clean command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="clean" name="clean" type="text" value="$(clean)" />
                        <input id="order-clean" class="task-order-input" type="number" value="$(clean_order)" oninput="setTaskOrder('$id', 'clean')" data-order="$(clean_order)" data-explicit-order="$(clean_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'clean')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'clean')">Down</button>
                    </div>
                </div>
            </div>

            <div id="row-terminal" class="task-order-row" style="order: $(terminal_order);">
                <div class="task-order-content">
                    <div class="setting-row-label">
                        <input id="default-terminal" type="checkbox" class="setting-default-checkbox" />
                        <label for="default-terminal">Terminal command</label>
                    </div>
                    <div class="task-command-input-row">
                        <input id="terminal" name="terminal" type="text" value="$(terminal)" />
                        <input id="order-terminal" class="task-order-input" type="number" value="$(terminal_order)" oninput="setTaskOrder('$id', 'terminal')" data-order="$(terminal_order)" data-explicit-order="$(terminal_has_explicit_order)" />
                        <button class="task-order-button" type="button" onclick="moveTaskUp('$id', 'terminal')">Up</button>
                        <button class="task-order-button" type="button" onclick="moveTaskDown('$id', 'terminal')">Down</button>
                    </div>
                </div>
            </div>

            <rust>
                for (index, action) in custom.iter().enumerate() {
                    let label = action.label.clone().unwrap_or_default();
                    let command = action.command.clone().unwrap_or_default();

                    if label.is_empty() || command.is_empty() {
                        continue;
                    }

                    let command_id = format!("custom-command-{}", index);
                    let default_id = format!("custom-default-{}", index);

                    <markup>
                        <div id="row-$(command_id)" class="task-order-row" style="order: $(task_order(label.as_str(), 7 + index));">
                            <div class="task-order-content">
                                <div class="setting-row-label">
                                    <input id="$(default_id)" type="checkbox" class="setting-default-checkbox" />
                                    <label for="$(default_id)">$(label)</label>
                                </div>
                                <div class="task-command-input-row">
                                    <input id="$(command_id)" name="$(command_id)" type="text" value="$(command)" />
                                    <input id="order-$(command_id)" class="task-order-input" type="number" value="$(task_order(label.as_str(), 7 + index))" oninput="setTaskOrder('$id', '$command_id')" data-order="$(task_order(label.as_str(), 7 + index))" data-explicit-order="$(order.contains_key(label.as_str()))" />
                                    <button class="task-order-button" type="button" onclick="moveTaskUp('$id', '$command_id')">Up</button>
                                    <button class="task-order-button" type="button" onclick="moveTaskDown('$id', '$command_id')">Down</button>
                                </div>
                            </div>
                        </div>
                    </markup>
                }
            </rust>
            </div>

            <p>
                <label for="custom">Custom Command</label>
                <div class="custom-command-inputs">
                    <input id="custom-label" name="custom-label" type="text" value="" placeholder="Label" />
                    <input id="custom-command" name="custom-command" type="text" value="" placeholder="Command" />
                </div>
                <button onclick="addCustomCommand('$id')">Add Custom Command</button>
            </p>

            <button onclick="saveSettings('$id')">Save</button>
        }};

        element.set_inner_html(content.as_str());

        for task in &tasks {
            let selector = format!("#default-{}", task);
            set_checkbox_checked(selector.as_str(), true);
        }

        for (index, command) in custom.iter().enumerate() {
            if let Some(label) = command.label.as_deref() {
                let label = label.trim();

                if tasks.iter().any(|task| task == label) {
                    let selector = format!("#custom-default-{}", index);
                    set_checkbox_checked(selector.as_str(), true);
                }
            }
        }

        reorder_task_order_list();
    }
}

#[callback("addCustomCommand")]
async fn add_cusom_command(id: String) {
    let label = try_get_value("#custom-label");
    let command = try_get_value("#custom-command");

    if label.is_empty() || command.is_empty() {
        log!("Not a full command.");
        return;
    }

    let component = SettingsComponent::get_component(&id);

    let Some(component) = component else {
        log!("No component");
        return;
    };

    {
        let mut inner = component.borrow_mut();
        inner.custom.push(CustomCommand {
            label: Some(label),
            command: Some(command),
        });
    }

    component.render();
}

#[callback("saveSettings")]
async fn save_settings(id: String) {
    let Some(component) = SettingsComponent::get_component(&id) else {
        log!("No component");
        return;
    };

    let project: Option<String> = component.get_attribute("project");
    let Some(project) = project else {
        log!("Project is not set in settings.");
        return;
    };

    let git: String = try_get_value("#git");
    let clone: String = try_get_value("#clone");
    let build: String = try_get_value("#build");
    let test: String = try_get_value("#test");
    let release: String = try_get_value("#release");
    let clean: String = try_get_value("#clean");
    let terminal: String = try_get_value("#terminal");
    let mut ordered_tasks = Vec::<(String, i32, usize)>::new();
    let mut task_sequence = 0_usize;
    let mut order = HashMap::<String, i32>::new();

    let git_order = try_get_i32_value("#order-git").unwrap_or(1);
    if try_has_explicit_order("#order-git") {
        order.insert("git".to_string(), git_order);
    }
    if try_is_checked("#default-git") && !git.trim().is_empty() {
        ordered_tasks.push(("git".to_string(), git_order, task_sequence));
        task_sequence += 1;
    }

    let clone_order = try_get_i32_value("#order-clone").unwrap_or(2);
    if try_has_explicit_order("#order-clone") {
        order.insert("clone".to_string(), clone_order);
    }
    if try_is_checked("#default-clone") && !clone.trim().is_empty() {
        ordered_tasks.push(("clone".to_string(), clone_order, task_sequence));
        task_sequence += 1;
    }

    let build_order = try_get_i32_value("#order-build").unwrap_or(3);
    if try_has_explicit_order("#order-build") {
        order.insert("build".to_string(), build_order);
    }
    if try_is_checked("#default-build") && !build.trim().is_empty() {
        ordered_tasks.push(("build".to_string(), build_order, task_sequence));
        task_sequence += 1;
    }

    let test_order = try_get_i32_value("#order-test").unwrap_or(4);
    if try_has_explicit_order("#order-test") {
        order.insert("test".to_string(), test_order);
    }
    if try_is_checked("#default-test") && !test.trim().is_empty() {
        ordered_tasks.push(("test".to_string(), test_order, task_sequence));
        task_sequence += 1;
    }

    let release_order = try_get_i32_value("#order-release").unwrap_or(5);
    if try_has_explicit_order("#order-release") {
        order.insert("release".to_string(), release_order);
    }
    if try_is_checked("#default-release") && !release.trim().is_empty() {
        ordered_tasks.push(("release".to_string(), release_order, task_sequence));
        task_sequence += 1;
    }

    let clean_order = try_get_i32_value("#order-clean").unwrap_or(6);
    if try_has_explicit_order("#order-clean") {
        order.insert("clean".to_string(), clean_order);
    }
    if try_is_checked("#default-clean") && !clean.trim().is_empty() {
        ordered_tasks.push(("clean".to_string(), clean_order, task_sequence));
        task_sequence += 1;
    }

    let terminal_order = try_get_i32_value("#order-terminal").unwrap_or(7);
    if try_has_explicit_order("#order-terminal") {
        order.insert("terminal".to_string(), terminal_order);
    }
    if try_is_checked("#default-terminal") && !terminal.trim().is_empty() {
        ordered_tasks.push(("terminal".to_string(), terminal_order, task_sequence));
        task_sequence += 1;
    }

    let custom_commands = {
        let mut inner = component.borrow_mut();

        let updated_commands = inner
            .custom
            .iter()
            .enumerate()
            .filter_map(|(index, command)| {
                let label = command.label.as_deref()?.trim();
                if label.is_empty() {
                    return None;
                }

                let selector = format!("#custom-command-{}", index);
                let input_command = try_get_value(selector.as_str()).trim().to_string();

                if input_command.is_empty() {
                    return None;
                }

                let checkbox_selector = format!("#custom-default-{}", index);
                let order_selector = format!("#order-custom-command-{}", index);
                let command_order =
                    try_get_i32_value(order_selector.as_str()).unwrap_or(100 + index as i32);
                if try_has_explicit_order(order_selector.as_str()) {
                    order.insert(label.to_string(), command_order);
                }

                if try_is_checked(checkbox_selector.as_str()) {
                    ordered_tasks.push((label.to_string(), command_order, task_sequence));
                    task_sequence += 1;
                }

                Some(CustomCommand {
                    label: Some(label.to_string()),
                    command: Some(input_command),
                })
            })
            .collect::<Vec<CustomCommand>>();

        inner.custom = updated_commands.clone();
        updated_commands
    };

    ordered_tasks.sort_by(|(task_a, order_a, index_a), (task_b, order_b, index_b)| {
        order_a
            .cmp(order_b)
            .then(index_a.cmp(index_b))
            .then(task_a.cmp(task_b))
    });

    let tasks = ordered_tasks
        .into_iter()
        .map(|(task, _, _)| task)
        .collect::<Vec<String>>();

    let settings = BuildCommands {
        git: Some(git.clone()),
        clone: Some(clone.clone()),
        build: Some(build.clone()),
        test: Some(test.clone()),
        release: Some(release.clone()),
        clean: Some(clean.clone()),
        terminal: Some(terminal.clone()),
        custom_comands: Some(custom_commands.clone()),
        tasks: Some(tasks.clone()),
        order: Some(order.clone()),
        run_all: Some(tasks.clone()),
    };

    let endpont = format!("/save-settings?project={}", project);
    let result = match Request::post(endpont.as_str()).json(&settings) {
        Ok(request) => request.send().await,
        Err(_) => {
            try_inner_text("#save-status", "Failed to build save request.");
            return;
        }
    };

    if result.is_ok() {
        if let Some(element) = get_element_by_id(&id) {
            let custom_json =
                serde_json::to_string(&custom_commands).unwrap_or_else(|_| "[]".to_string());
            let tasks_json = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
            let order_json = serde_json::to_string(&order).unwrap_or_else(|_| "{}".to_string());

            let _ = element.set_attribute("git", git.as_str());
            let _ = element.set_attribute("clone", clone.as_str());
            let _ = element.set_attribute("build", build.as_str());
            let _ = element.set_attribute("test", test.as_str());
            let _ = element.set_attribute("release", release.as_str());
            let _ = element.set_attribute("clean", clean.as_str());
            let _ = element.set_attribute("terminal", terminal.as_str());
            let _ = element.set_attribute("custom", custom_json.as_str());
            let _ = element.set_attribute("tasks", tasks_json.as_str());
            let _ = element.set_attribute("order", order_json.as_str());
        }

        try_inner_text("#save-status", "Settings saved.");
    } else {
        try_inner_text("#save-status", "Failed to save settings.");
    }

    component.render();
}

#[callback("moveTaskUp")]
fn move_task_up(_id: String, task_key: String) {
    adjust_task_order(task_key.as_str(), -1);
}

#[callback("moveTaskDown")]
fn move_task_down(_id: String, task_key: String) {
    adjust_task_order(task_key.as_str(), 1);
}

#[callback("setTaskOrder")]
fn set_task_order(_id: String, task_key: String) {
    sync_task_order_input(task_key.as_str());
}

fn adjust_task_order(task_key: &str, delta: i32) {
    let selector = format!("#order-{}", task_key);
    let Some(current) = try_get_i32_value(selector.as_str()) else {
        return;
    };

    let next = (current + delta).max(1);
    if next == current {
        return;
    }

    try_set_value(selector.as_str(), next.to_string().as_str());
    sync_task_order_input(task_key);
}

fn sync_task_order_input(task_key: &str) {
    let selector = format!("#order-{}", task_key);
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Ok(Some(current_element)) = document.query_selector(selector.as_str()) else {
        return;
    };

    let Ok(current_input) = current_element.dyn_into::<HtmlInputElement>() else {
        return;
    };

    let previous = current_input
        .get_attribute("data-order")
        .and_then(|value| value.parse::<i32>().ok())
        .or_else(|| current_input.value().parse::<i32>().ok())
        .unwrap_or(1)
        .max(1);

    let next = current_input
        .value()
        .parse::<i32>()
        .ok()
        .unwrap_or(previous)
        .max(1);

    if next == previous {
        current_input.set_value(next.to_string().as_str());
        let _ = current_input.set_attribute("data-order", next.to_string().as_str());
        update_task_row_order(task_key, next);
        return;
    }

    if let Ok(order_inputs) = document.query_selector_all("input.task-order-input") {
        for index in 0..order_inputs.length() {
            let Some(node) = order_inputs.item(index) else {
                continue;
            };

            let Ok(input) = node.dyn_into::<HtmlInputElement>() else {
                continue;
            };

            if input.id() == current_input.id() {
                continue;
            }

            if input
                .value()
                .parse::<i32>()
                .ok()
                .is_some_and(|value| value == next)
            {
                input.set_value(previous.to_string().as_str());
                let _ = input.set_attribute("data-order", previous.to_string().as_str());
                let _ = input.set_attribute("data-explicit-order", "true");

                if let Some(other_task_key) = input.id().strip_prefix("order-") {
                    update_task_row_order(other_task_key, previous);
                }

                break;
            }
        }
    }

    current_input.set_value(next.to_string().as_str());
    let _ = current_input.set_attribute("data-order", next.to_string().as_str());
    let _ = current_input.set_attribute("data-explicit-order", "true");
    update_task_row_order(task_key, next);
}

fn update_task_row_order(task_key: &str, order: i32) {
    let row_selector = format!("#row-{}", task_key);
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Ok(Some(element)) = document.query_selector(row_selector.as_str()) else {
        return;
    };

    let _ = element.set_attribute("style", format!("order: {};", order).as_str());
    reorder_task_order_list();
}

fn reorder_task_order_list() {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Ok(Some(task_order_list)) = document.query_selector("#task-order-list") else {
        return;
    };

    let Ok(rows) = task_order_list.query_selector_all(".task-order-row") else {
        return;
    };

    let mut ordered_rows = Vec::<(bool, i32, String, u32, Element)>::new();

    for index in 0..rows.length() {
        let Some(node) = rows.item(index) else {
            continue;
        };

        let Ok(row) = node.dyn_into::<Element>() else {
            continue;
        };

        let row_id = row.id();
        let Some(id) = row_id.strip_prefix("row-") else {
            continue;
        };

        let input_selector = format!("#order-{}", id);
        let (has_explicit_order, order) = task_order_list
            .query_selector(input_selector.as_str())
            .ok()
            .flatten()
            .and_then(|input| input.dyn_into::<HtmlInputElement>().ok())
            .map(|input| {
                let has_explicit_order = input
                    .get_attribute("data-explicit-order")
                    .map(|value| value == "true")
                    .unwrap_or(false);
                let order = input
                    .value()
                    .parse::<i32>()
                    .ok()
                    .unwrap_or((index as i32) + 1)
                    .max(1);
                (has_explicit_order, order)
            })
            .unwrap_or((false, (index as i32) + 1));

        let sort_text = row.text_content().unwrap_or_default();
        ordered_rows.push((has_explicit_order, order, sort_text, index, row));
    }

    ordered_rows.sort_by(
        |(explicit_a, order_a, text_a, index_a, _), (explicit_b, order_b, text_b, index_b, _)| {
            explicit_b
                .cmp(explicit_a)
                .then(order_a.cmp(order_b))
                .then(text_a.cmp(text_b))
                .then(index_a.cmp(index_b))
        },
    );

    for (_, _, _, _, row) in ordered_rows {
        let _ = task_order_list.append_child(&row);
    }
}

fn try_get_i32_value(selector: &str) -> Option<i32> {
    let value = try_get_value(selector);
    value.trim().parse::<i32>().ok()
}

fn try_has_explicit_order(selector: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };

    let Some(document) = window.document() else {
        return false;
    };

    let Ok(Some(element)) = document.query_selector(selector) else {
        return false;
    };

    element
        .dyn_into::<HtmlInputElement>()
        .ok()
        .and_then(|input| input.get_attribute("data-explicit-order"))
        .is_some_and(|value| value == "true")
}

fn try_is_checked(selector: &str) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };

    let Some(document) = window.document() else {
        return false;
    };

    let Ok(Some(element)) = document.query_selector(selector) else {
        return false;
    };

    element
        .dyn_into::<HtmlInputElement>()
        .ok()
        .is_some_and(|input| input.checked())
}

fn try_set_value(selector: &str, value: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Ok(Some(element)) = document.query_selector(selector) else {
        return;
    };

    if let Ok(input) = element.dyn_into::<HtmlInputElement>() {
        input.set_value(value);
    }
}

fn set_checkbox_checked(selector: &str, checked: bool) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let Some(document) = window.document() else {
        return;
    };

    let Ok(Some(element)) = document.query_selector(selector) else {
        return;
    };

    if let Ok(input) = element.dyn_into::<HtmlInputElement>() {
        input.set_checked(checked);
    }
}
