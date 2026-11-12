use crate::rest::BuildCommands;

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

            <button onclick="saveSettings()">Save</button>
        }};

        element.set_inner_html(content.as_str());
    }
}

#[callback("saveSettings")]
async fn save_settings() {
    let project: String = try_get_value("project");
    let git: String = try_get_value("git");
    let build: String = try_get_value("build");
    let test: String = try_get_value("test");
    let release: String = try_get_value("release");
    let clean: String = try_get_value("clean");

    let settings = BuildCommands {
        git: Some(git),
        build: Some(build),
        test: Some(test),
        release: Some(release),
        clean: Some(clean),
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
}
