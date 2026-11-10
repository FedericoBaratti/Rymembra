//! L'eseguibile di Windows: `rymembra.exe`.
//!
//! Tutta l'applicazione sta nella libreria (`lib.rs`), perché è da lì che entra
//! anche Android, che un `main` non ce l'ha. Qui resta quel che riguarda
//! soltanto l'eseguibile.

// Senza, in rilascio si aprirebbe una finestra di console dietro l'applicazione.
// In sviluppo la si vuole: è dove finiscono i messaggi di avvio.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rymembra_lib::run();
}
