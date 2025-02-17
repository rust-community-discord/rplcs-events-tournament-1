use crate::wasm_instance_wrapper::WasmInstanceWrapper;
use anyhow::{Context, Result};
use rplcs_events::tournament_1::{ChoiceResponse, MapNodeType, PlayerChoices, PlayerState};
use wasmtime::{Engine, Linker, Module, Store};

#[derive(Clone)]
pub struct Submission {
    name: String,
    instance: WasmInstanceWrapper<()>,
    player_state: PlayerState,
}

impl Submission {
    pub fn new(name: String, wasm_bytes: &[u8]) -> Result<Self> {
        let wrapper = WasmInstanceWrapper::new(wasm_bytes).context("Failed to create instance")?;

        Ok(Submission {
            name,
            instance: wrapper,
            player_state: PlayerState {
                health: 3,
                max_health: 3,
                power: 5,
            },
        })
    }

    fn call_function<T, U>(&self, function_name: &str, input: T) -> Result<U>
    where
        T: serde::Serialize,
        U: serde::de::DeserializeOwned,
    {
        let input_json = serde_json::to_string(&input).context("Failed to serialize input")?;
        let input_bytes = input_json.as_bytes();

        let allocated_string = self.instance.allocate_string(input_bytes)?;

        let function = self
            .instance
            .instance
            .get_func(&mut self.instance.store, function_name)
            .context("Failed to get function")?;


    }

    fn call_function_0<U>(&self, function_name: &str) -> U
    where
        U: serde::de::DeserializeOwned,
    {
        let output_json = self.instance.call_function_0(function_name);
        serde_json::from_str(&output_json).unwrap()
    }

    pub fn get_choices(&self, choices: &PlayerChoices) -> ChoiceResponse {
        self.call_function("get_choices", choices)
    }

    pub fn get_gamble_choice(&self) -> MapNodeType {
        self.call_function_0("get_gamble_choice")
    }

    pub fn get_fight_choice(&self, fight_info: &PlayerState) -> MapNodeType {
        self.call_function("get_fight_choice", fight_info)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn player_state(&self) -> &PlayerState {
        &self.player_state
    }

    pub fn player_state_mut(&mut self) -> &mut PlayerState {
        &mut self.player_state
    }
}
