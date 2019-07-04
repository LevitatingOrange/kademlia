#![feature(trait_alias)]
#![feature(core_intrinsics)]

mod bucket;
mod codec;
mod controller;
mod util;

pub use controller::{create_controller, ControllerMessage};
pub use util::Key;

// #[cfg(test)]
// mod tests {
//     #[test]
//     fn it_works() {
//         assert_eq!(2 + 2, 4);
//     }
// }
