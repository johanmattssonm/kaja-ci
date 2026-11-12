use kaja_web::prelude::*;

mod buildview;
mod color;
mod settings;

pub mod rest;

#[wasm_bindgen(start)]
pub fn init() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console_error_panic_hook::set_once();
    init_callbacks();
}
