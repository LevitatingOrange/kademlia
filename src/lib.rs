#![feature(trait_alias)]
#![feature(core_intrinsics)]

mod bucket;
mod codec;
mod controller;
mod util;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
