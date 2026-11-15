use crate::rest::{BuildCommands, CustomCommand};

use gloo_net::http::Request;
use kaja_web::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;

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

        let content = html! {{
            <p>$(project)</p>
            <p id="save-status"></p>

            <input type="hidden" name="project" value="$(project)" />

            <p>
                <div class="setting-row-label">
                    <input id="default-git" type="checkbox" class="setting-default-checkbox" />
                    <label for="default-git">Git fetch command</label>
                </div>
                <input id="git" name="git" type="text" value="$(git)" />
            </p>

            <p>
                <div class="setting-row-label">
                    <input id="default-build" type="checkbox" class="setting-default-checkbox" />
                    <label for="default-build">Build command</label>
                </div>
                <input id="build" name="build" type="text" value="$(build)" />
            </p>

            <p>
                <div class="setting-row-label">
                    <input id="default-test" type="checkbox" class="setting-default-checkbox" />
                    <label for="default-test">Test command</label>
                </div>
                <input id="test" name="test" type="text" value="$(test)" />
            </p>

            <p>
                <div class="setting-row-label">
                    <input id="default-release" type="checkbox" class="setting-default-checkbox" />
                    <label for="default-release">Release command</label>
                </div>
                <input id="release" name="release" type="text" value="$(release)" />
            </p>

            <p>
                <div class="setting-row-label">
                    <input id="default-clean" type="checkbox" class="setting-default-checkbox" />
                    <label for="default-clean">Clean command</label>
                </div>
                <input id="clean" name="clean" type="text" value="$(clean)" />
            </p>

            <p>
                <div class="setting-row-label">
                    <input id="default-terminal" type="checkbox" class="setting-default-checkbox" />
                    <label for="default-terminal">Terminal command</label>
                </div>
                <input id="terminal" name="terminal" type="text" value="$(terminal)" />
            </p>

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
                        <p>
                            <div class="setting-row-label">
                                <input id="$(default_id)" type="checkbox" class="setting-default-checkbox" />
                                <label for="$(default_id)">$(label)</label>
                            </div>
                            <input id="$(command_id)" name="$(command_id)" type="text" value="$(command)" />
                        </p>
                    </markup>
                }
            </rust>

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
    let mut tasks = Vec::new();

    if try_is_checked("#default-git") && !git.trim().is_empty() {
        tasks.push("git".to_string());
    }

    if try_is_checked("#default-clone") && !clone.trim().is_empty() {
        tasks.push("clone".to_string());
    }

    if try_is_checked("#default-build") && !build.trim().is_empty() {
        tasks.push("build".to_string());
    }

    if try_is_checked("#default-test") && !test.trim().is_empty() {
        tasks.push("test".to_string());
    }

    if try_is_checked("#default-release") && !release.trim().is_empty() {
        tasks.push("release".to_string());
    }

    if try_is_checked("#default-clean") && !clean.trim().is_empty() {
        tasks.push("clean".to_string());
    }

    if try_is_checked("#default-terminal") && !terminal.trim().is_empty() {
        tasks.push("terminal".to_string());
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
                if try_is_checked(checkbox_selector.as_str()) {
                    tasks.push(label.to_string());
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

            let _ = element.set_attribute("git", git.as_str());
            let _ = element.set_attribute("clone", clone.as_str());
            let _ = element.set_attribute("build", build.as_str());
            let _ = element.set_attribute("test", test.as_str());
            let _ = element.set_attribute("release", release.as_str());
            let _ = element.set_attribute("clean", clean.as_str());
            let _ = element.set_attribute("terminal", terminal.as_str());
            let _ = element.set_attribute("custom", custom_json.as_str());
            let _ = element.set_attribute("tasks", tasks_json.as_str());
        }

        try_inner_text("#save-status", "Settings saved.");
    } else {
        try_inner_text("#save-status", "Failed to save settings.");
    }

    component.render();
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
