use crate::rest::{BuildCommands, CustomCommand};

use gloo_net::http::Request;
use kaja_web::prelude::*;
use wasm_bindgen_futures::spawn_local;

#[component("project-status")]
struct ProjectStatus {
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

impl Component for ProjectStatusComponent {
    fn connected(&self) {
        let id = self.get_id();
        log!("Connected:", &id);
        self.render();

        let project: String = self.get_attribute("project").unwrap_or_default();

        spawn_local(async move {
            let settings = load_settings(&project).await;
            let element = get_element_by_id(&id);

            if element.is_none() {
                log!("Missing component.");
                return;
            }

            if settings.is_none() {
                log!("Missing settings.");
                return;
            }

            let element = element.unwrap();
            let settings = settings.unwrap();

            let git = settings.git.unwrap_or_default();
            let build = settings.build.unwrap_or_default();
            let test = settings.test.unwrap_or_default();
            let release = settings.release.unwrap_or_default();
            let clean = settings.clean.unwrap_or_default();
            let terminal = settings.terminal.unwrap_or_default();
            let custom = settings.custom_comands.unwrap_or_default();
            let custom_json = serde_json::to_string(&custom).unwrap_or_else(|_| "[]".to_string());

            element.set_attribute("git", git.as_str());
            element.set_attribute("build", build.as_str());
            element.set_attribute("test", test.as_str());
            element.set_attribute("release", release.as_str());
            element.set_attribute("clean", clean.as_str());
            element.set_attribute("terminal", terminal.as_str());
            element.set_attribute("custom", custom_json.as_str());
        });
    }

    fn disconnected(&self) {
        log!("Disconnected:", self.get_id());
    }

    fn attribute_changed(&self, name: &str, _old_value: &str, _new_value: &str) {
        if name != "project" && name != "custom" {
            return;
        }

        self.render();
    }
}

impl ProjectStatusComponent {
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
        let custom_actions: Vec<CustomCommand> = self.get_attribute("custom").unwrap_or_default();

        let content = html! {{
            <p>$project branch <build-status project="$project"></build-status></p>
            <div class="project-actions">
                <button class="project-action-button" onclick="runAll('$project')">Run</button>

                <rust>
                    let attr: String = self.get_attribute("git").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="goTo('/fetch?project=$project')">Git Fetch</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("build").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="goTo('/build?project=$project')">Build</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("test").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="goTo('/test?project=$project')">Test</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("release").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="goTo('/release?project=$project')">Release</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("clean").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="goTo('/clean?project=$project')">Clean</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("terminal").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="goTo('/terminal?project=$project')">Terminal</button>
                        </markup>
                    }
                </rust>

                <rust>
                    for custom in &custom_actions {
                        if let Some(label) = custom.label.as_ref() {
                            let label = label.trim();

                            if !label.is_empty() {
                                <markup>
                                    <button class="project-action-button" onclick="goTo('/custom?project=$(project)&custom_label=$label')">$label</button>
                                </markup>
                            }
                        }
                    }
                </rust>

                <button class="project-action-button" onclick="goTo('/settings?project=$project')">Settings</button>
                <button class="project-action-button" onclick="goTo('/history?project=$project')">History</button>
            </div>
            </div>
        }};

        element.set_inner_html(content.as_str());
    }
}

#[callback("goTo")]
fn go_to(url: String) {
    let Some(window) = web_sys::window() else {
        return;
    };

    let _ = window.location().set_href(url.as_str());
}

#[callback("runAll")]
async fn run_all(project: String) {
    let endpoint = format!("/run-all?project={}", project);
    match Request::get(endpoint.as_str()).send().await {
        Ok(_) => {
            log!("Running all tasks.");
        }
        Err(_) => {
            log!("Failed to run all tasks.");
        }
    };
}

async fn load_settings(project: &str) -> Option<BuildCommands> {
    let endpoint = format!("/load-settings?project={}", project);

    let response = match Request::get(endpoint.as_str()).send().await {
        Ok(response) => response,
        Err(_) => {
            log!("Failed to load settings.");
            return None;
        }
    };

    let body = match response.text().await {
        Ok(body) => body,
        Err(_) => {
            log!("#save-status", "Failed to read settings response.");
            return None;
        }
    };

    match serde_json::from_str::<BuildCommands>(body.as_str()) {
        Ok(commands) => return Some(commands),
        Err(_) => {
            log!("#save-status", "Failed to parse settings response.");
            return None;
        }
    }
}
