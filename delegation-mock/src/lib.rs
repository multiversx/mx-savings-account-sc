#![no_std]

elrond_wasm::imports!();

#[elrond_wasm::contract]
pub trait DelegationMock {
    #[init]
    fn init(&self) {}
}
