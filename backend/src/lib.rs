// wit-bindgen emits the canonical ABI wrappers. Handwritten Guest code forbids unsafe.
mod bindings {
    wit_bindgen::generate!({ path: "wit", world: "background-integration-v1", generate_all });
    #[cfg(target_arch = "wasm32")]
    use super::application::Guest;
    #[cfg(target_arch = "wasm32")]
    export!(Guest with_types_in self);
}

mod application;

pub use application::extension_id;
