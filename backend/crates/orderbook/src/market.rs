//! Per-symbol engine registry. One `MatchingEngine` per symbol.

use common::Symbol;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::engine::MatchingEngine;

#[derive(Clone)]
pub struct SymbolRegistry {
    inner: Arc<Mutex<HashMap<Symbol, Arc<Mutex<MatchingEngine>>>>>,
}

impl SymbolRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Look up the engine for `symbol` without creating it. Returns `None`
    /// if the engine is not registered. Use this for read endpoints and for
    /// the Redis consumer's dispatch so an unknown symbol doesn't spawn an
    /// empty book.
    pub async fn get(&self, symbol: &Symbol) -> Option<Arc<Mutex<MatchingEngine>>> {
        let map = self.inner.lock().await;
        map.get(symbol).cloned()
    }

    pub async fn get_or_create(&self, symbol: Symbol) -> Arc<Mutex<MatchingEngine>> {
        let mut map = self.inner.lock().await;
        if let Some(e) = map.get(&symbol) {
            return e.clone();
        }
        let engine = Arc::new(Mutex::new(MatchingEngine::new(symbol.clone())));
        map.insert(symbol, engine.clone());
        engine
    }

    pub async fn list(&self) -> Vec<Symbol> {
        self.inner.lock().await.keys().cloned().collect()
    }
}

impl Default for SymbolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
