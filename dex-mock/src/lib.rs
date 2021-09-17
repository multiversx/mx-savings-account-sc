#![no_std]

elrond_wasm::imports!();

#[elrond_wasm::contract]
pub trait DexMock {
    #[init]
    fn init(&self) {}
}
