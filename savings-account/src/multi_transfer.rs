elrond_wasm::imports!();
elrond_wasm::derive_imports!();

use elrond_wasm::{HexCallDataSerializer, String, abi::{OutputAbi, TypeAbi, TypeDescriptionContainer}, elrond_codec::TopEncode};

// Temporary until the next version is released
// In 0.19, this entire file will be removed

pub const ESDT_MULTI_TRANSFER_STRING: &[u8] = b"MultiESDTNFTTransfer";

extern "C" {
    fn bigIntNew(value: i64) -> i32;
    fn bigIntUnsignedByteLength(x: i32) -> i32;
    fn bigIntGetUnsignedBytes(reference: i32, byte_ptr: *mut u8) -> i32;

    fn getNumESDTTransfers() -> i32;
    fn bigIntGetESDTCallValueByIndex(dest: i32, index: i32);
    fn getESDTTokenNameByIndex(resultOffset: *const u8, index: i32) -> i32;
    fn getESDTTokenNonceByIndex(index: i32) -> i64;
    fn getESDTTokenTypeByIndex(index: i32) -> i32;
}

#[derive(TypeAbi, TopEncode, TopDecode, NestedEncode, NestedDecode, Clone)]
pub struct EsdtTokenPayment<BigUint: BigUintApi> {
    pub token_type: EsdtTokenType,
    pub token_name: TokenIdentifier,
    pub token_nonce: u64,
    pub amount: BigUint,
}

pub struct MultiTransferAsync<SA>
where
    SA: SendApi + 'static,
{
    api: SA,
    hex_data: HexCallDataSerializer,
    callback_data: HexCallDataSerializer,
}

impl<SA: SendApi + 'static> MultiTransferAsync<SA> {
    pub fn new(
        api: SA,
        to: Address,
        endpoint_name: &[u8],
        transfers: Vec<EsdtTokenPayment<SA::AmountType>>,
    ) -> Self {
        let mut hex_data = HexCallDataSerializer::new(ESDT_MULTI_TRANSFER_STRING);

        hex_data.push_argument_bytes(to.as_bytes());
        hex_data.push_argument_bytes(&transfers.len().to_be_bytes()[..]);

        for transf in transfers {
            hex_data.push_argument_bytes(transf.token_name.as_esdt_identifier());
            hex_data.push_argument_bytes(&transf.token_nonce.to_be_bytes()[..]);
            hex_data.push_argument_bytes(transf.amount.to_bytes_be().as_slice());
        }

        if !endpoint_name.is_empty() {
            hex_data.push_argument_bytes(endpoint_name);
        }

        Self {
            api,
            hex_data,
            callback_data: HexCallDataSerializer::new(&[]),
        }
    }

    pub fn add_endpoint_arg<T: TopEncode>(&mut self, arg: &T) {
        let mut encoded = Vec::new();
        let _ = arg.top_encode(&mut encoded);
        self.hex_data.push_argument_bytes(encoded.as_slice());
    }

    pub fn add_callback(&mut self, callback_name: &[u8]) {
        self.callback_data = HexCallDataSerializer::new(callback_name);
    }

    pub fn add_callback_arg<T: TopEncode>(&mut self, arg: &T) {
        let mut encoded = Vec::new();
        let _ = arg.top_encode(&mut encoded);
        self.callback_data.push_argument_bytes(encoded.as_slice());
    }
}

impl<SA> TypeAbi for MultiTransferAsync<SA>
where
    SA: SendApi + 'static,
{
    fn type_name() -> String {
        "MultiTransferAsync".into()
    }

    /// No ABI output.
    fn output_abis(_: &[&'static str]) -> Vec<OutputAbi> {
        Vec::new()
    }

    fn provide_type_descriptions<TDC: TypeDescriptionContainer>(_: &mut TDC) {}
}

impl<SA: SendApi + 'static> EndpointResult for MultiTransferAsync<SA> {
    type DecodeAs = ();

    #[inline]
    fn finish<FA>(&self, _api: FA) {
        self.api
            .storage_store_tx_hash_key(self.callback_data.as_slice());

        self.api.async_call_raw(
            &self.api.get_sc_address(),
            &SA::AmountType::zero(),
            self.hex_data.as_slice(),
        );
    }
}

#[elrond_wasm::module]
pub trait MultiTransferModule {
    fn esdt_num_transfers(&self) -> usize {
        unsafe { getNumESDTTransfers() as usize }
    }

    fn esdt_value_by_index(&self, index: usize) -> Self::BigUint {
        unsafe {
            let value_handle = bigIntNew(0);
            bigIntGetESDTCallValueByIndex(value_handle, index as i32);

            let mut value_buffer = [0u8; 64];
            let value_byte_len = bigIntUnsignedByteLength(value_handle) as usize;
            bigIntGetUnsignedBytes(value_handle, value_buffer.as_mut_ptr());

            Self::BigUint::from_bytes_be(&value_buffer[..value_byte_len])
        }
    }

    fn token_by_index(&self, index: usize) -> TokenIdentifier {
        unsafe {
            let mut name_buffer = [0u8; 32];
            let name_len = getESDTTokenNameByIndex(name_buffer.as_mut_ptr(), index as i32);
            if name_len == 0 {
                TokenIdentifier::egld()
            } else {
                TokenIdentifier::from(&name_buffer[..name_len as usize])
            }
        }
    }

    fn esdt_token_nonce_by_index(&self, index: usize) -> u64 {
        unsafe { getESDTTokenNonceByIndex(index as i32) as u64 }
    }

    fn esdt_token_type_by_index(&self, index: usize) -> EsdtTokenType {
        unsafe { (getESDTTokenTypeByIndex(index as i32) as u8).into() }
    }

    fn get_all_esdt_transfers(&self) -> Vec<EsdtTokenPayment<Self::BigUint>> {
        let num_transfers = self.esdt_num_transfers();
        let mut transfers = Vec::with_capacity(num_transfers);

        for i in 0..num_transfers {
            let token_type = self.esdt_token_type_by_index(i);
            let token_name = self.token_by_index(i);
            let token_nonce = self.esdt_token_nonce_by_index(i);
            let amount = self.esdt_value_by_index(i);

            transfers.push(EsdtTokenPayment {
                token_type,
                token_name,
                token_nonce,
                amount,
            });
        }

        transfers
    }

    fn multi_transfer_via_execute_on_dest_context(
        &self,
        to: &Address,
        transfers: &[EsdtTokenPayment<Self::BigUint>],
        endpoint_name: &BoxedBytes,
        args: &[BoxedBytes],
    ) -> Vec<BoxedBytes> {
        let mut arg_buffer = ArgBuffer::new();
        arg_buffer.push_argument_bytes(to.as_bytes());
        arg_buffer.push_argument_bytes(&transfers.len().to_be_bytes()[..]);

        for transf in transfers {
            arg_buffer.push_argument_bytes(transf.token_name.as_esdt_identifier());
            arg_buffer.push_argument_bytes(&transf.token_nonce.to_be_bytes()[..]);
            arg_buffer.push_argument_bytes(transf.amount.to_bytes_be().as_slice());
        }

        if !endpoint_name.is_empty() {
            arg_buffer.push_argument_bytes(endpoint_name.as_slice());

            for arg in args {
                arg_buffer.push_argument_bytes(arg.as_slice());
            }
        }

        self.send().execute_on_dest_context_raw(
            self.blockchain().get_gas_left(),
            &self.blockchain().get_sc_address(),
            &Self::BigUint::zero(),
            ESDT_MULTI_TRANSFER_STRING,
            &arg_buffer,
        )
    }
}
