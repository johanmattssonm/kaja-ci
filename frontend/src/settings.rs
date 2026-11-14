use crate::rest::{BuildCommands, CustomCommand};

use gloo_net::http::Request;
use kaja_web::prelude::*;

#[component("project-settings")]
struct Settings {
    #[attribute]
    pub project: String,
    #[attribute]
    pub git: String,
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
        let build: String = self.get_attribute("build").unwrap_or_default();
        let test: String = self.get_attribute("test").unwrap_or_default();
        let release: String = self.get_attribute("release").unwrap_or_default();
        let clean: String = self.get_attribute("clean").unwrap_or_default();
        let terminal: String = self.get_attribute("terminal").unwrap_or_default();
        let custom: Vec<CustomCommand> = self.get_attribute("custom").unwrap_or_default();

        let content = html! {{
            <p>$(project)</p>
            <p id="save-status"></p>

            <input type="hidden" name="project" value="$(project)" />

            <p>
                <label for="git">Git command</label><br />
                <input id="git" name="git" type="text" value="$(git)" />
            </p>

            <p>
                <label for="build">Build command</label><br />
                <input id="build" name="build" type="text" value="$(build)" />
            </p>

            <p>
                <label for="test">Test command</label><br />
                <input id="test" name="test" type="text" value="$(test)" />
            </p>

            <p>
                <label for="release">Release command</label><br />
                <input id="release" name="release" type="text" value="$(release)" />
            </p>

            <p>
                <label for="clean">Clean command</label><br />
                <input id="clean" name="clean" type="text" value="$(clean)" />
            </p>

            <p>
                <label for="terminal">Terminal command</label><br />
                <input id="terminal" name="terminal" type="text" value="$(terminal)" />
            </p>

            <rust>
                for (index, action) in custom.into_iter().enumerate() {
                    let label = action.label.unwrap_or_default();
                    let command = action.command.unwrap_or_default();

                    if label.is_empty() || command.is_empty() {
                        continue;
                    }

                    let command_id = format!("custom-command-{}", index);

                    <markup>
                        <p>
                            <label for="$(command_id)">$(label)</label><br />
                            <input id="$(command_id)" name="$(command_id)" type="text" value="$(command)" />
                        </p>
                    </markup>
                }
            </rust>

            <p>
                <label for="custom">Custom Command</label><br />
                <input id="custom-label" name="custom-label" type="text" value="" placeholder="Label" /><br>
                <input id="custom-command" name="custom-command" type="text" value="" placeholder="Command" />
                <button onclick="addCustomCommand('$id')">Add Custom Command</button>
            </p>

            <button onclick="saveSettings('$id')">Save</button>
        }};

        element.set_inner_html(content.as_str());
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
    let build: String = try_get_value("#build");
    let test: String = try_get_value("#test");
    let release: String = try_get_value("#release");
    let clean: String = try_get_value("#clean");
    let terminal: String = try_get_value("#terminal");
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
        git: Some(git),
        build: Some(build),
        test: Some(test),
        release: Some(release),
        clean: Some(clean),
        terminal: Some(terminal),
        custom_comands: Some(custom_commands),
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
        try_inner_text("#save-status", "Settings saved.");
    } else {
        try_inner_text("#save-status", "Failed to save settings.");
    }

    component.render();
}
