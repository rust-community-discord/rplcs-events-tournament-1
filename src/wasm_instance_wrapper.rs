use wasmtime::{Engine, Instance, Linker, Module, Store, Val, WasmParams, WasmResults};
use anyhow::{anyhow, Result, Context};
use rplcs_events::wasm_alloc::StringBuf;


const MEMORY: &str = "memory";

pub struct WasmInstanceWrapper<StoreType> {
    store: Store<StoreType>,
    instance: Instance,
}

impl<StoreType> WasmInstanceWrapper<StoreType>
where StoreType: Default {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self> {
        let engine = Engine::default();
        let module = Module::new(&engine, wasm_bytes).context("Failed to compile module")?;

        let mut store = Store::new(&engine, StoreType::default());

        let linker = Linker::new(&engine);

        let instance = linker
            .instantiate(&mut store, &module)
            .context("Failed to create instance")?;

        Ok(WasmInstanceWrapper {
            store,
            instance,
        })
    }

    pub fn allocate_string(&mut self, bytes: &[u8]) -> Result<u32> {
        if bytes.is_empty() {
            return Ok(0);
        }

        if bytes.len() > u32::MAX as usize {
            return Err(anyhow!("byte array is too large"));
        }

        let memory = self.instance
            .get_memory(&mut self.store, MEMORY)
            .context("expected memory not found")?;

        let alloc = self.instance
            .get_func(&mut self.store, "alloc")
            .context("expected alloc function not found")?;

        let alloc_params = [Val::I32(bytes.len() as i32)];

        let mut alloc_result = Vec::new();

        alloc.call(&mut self.store, &alloc_params, &mut alloc_result)?;

        let guest_ptr_offset = match alloc_result
            .get(0)
            .context("expected the result of the allocation to have one value")?
        {
            Val::I32(val) => *val as u32,
            _ => return Err(anyhow!("guest pointer must be Val::I32")),
        };

        if guest_ptr_offset < 0 {
            return Err(anyhow!("guest pointer must be non-negative"));
        }

        memory.write(&mut self.store, guest_ptr_offset as usize, bytes)
                .context("Failed to write to memory")?;

        Ok(guest_ptr_offset)
    }

    pub fn call_function(&self, name: &str, input: &str) -> Result<String> {
        let function = self
            .instance
            .get_func(&mut self.store, name)
            .context("Failed to get function")?;

        let input_bytes = self.allocate_string(input.as_bytes())?;

        let function_params = [Val::I32(input_bytes as i32)];

        let mut results = Vec::new();

        function
            .call(&mut self.store, &function_params, &mut results)
            .context("Failed to call function")
            .unwrap();

        // receive StringBuf from wasm

        let string_buf = StringBuf {
            ptr: results[0].unwrap_i32() as *const u8,
            len: results[1].unwrap_i32() as usize,
        };

        let returns = String::from_utf8(
            self.instance
                .get_memory(&mut self.store, MEMORY)
                .context("expected memory not found")?
                .read(&self.store, string_buf.ptr as usize, string_buf.len)
                .context("Failed to read from memory")?
        ).context("Failed to convert bytes to string")?;

        Ok(returns)
    }
}
