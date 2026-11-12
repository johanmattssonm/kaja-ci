use crate::rest::BuildCommands;

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

            element.set_attribute("git", git.as_str());
            element.set_attribute("build", build.as_str());
            element.set_attribute("test", test.as_str());
            element.set_attribute("release", release.as_str());
            element.set_attribute("clean", clean.as_str());
        });
    }

    fn disconnected(&self) {
        log!("Disconnected:", self.get_id());
    }

    fn attribute_changed(&self, _name: &str, _old_value: &str, _new_value: &str) {
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

        let content = html! {{
            <p>$project</p>
            <div class="project-actions">
                <rust>
                    let attr: String = self.get_attribute("git").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="window.location.href='/fetch?project=$project'">Git Fetch</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("build").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="window.location.href='/build?project=$project'">Build</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("test").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="window.location.href='/test?project=$project'">Test</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("release").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="window.location.href='/release?project=$project'">Release</button>
                        </markup>
                    }
                </rust>

                <rust>
                    let attr: String = self.get_attribute("clean").unwrap_or_default();

                    if !attr.is_empty() {
                        <markup>
                            <button class="project-action-button" onclick="window.location.href='/clean?project=$project'">Clean</button>
                        </markup>
                    }
                </rust>

                <button class="project-action-button" onclick="window.location.href='/settings?project=$project'">Settings</button>
            </div>
        }};

        element.set_inner_html(content.as_str());
    }
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
