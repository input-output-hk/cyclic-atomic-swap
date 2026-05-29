//! This module serves as the main entry point for the library, providing access to various submodules
//! that implement the core functionality. Below is a description of each module:
//!
//! - `dashboard`: (Optional, enabled with the "dashboard" feature) Contains the implementation for the
//!    dashboard functionality, providing tools for visualization and monitoring.
//!
//! - `blockchains`: Provides functionality related to blockchain operations, handling various blockchain
//!    implementations and their associated logic.
//!
//! - `config`: Manages application configuration, including configuration loading, parsing, and validation.
//!
//! - `cryptography`: Includes cryptographic utilities and algorithms used for secure operations such as
//!    encryption, hashing, and signatures.
//!
//! - `daemon`: Contains functionality for creating and managing daemon processes, ensuring the application
//!    can run as a background service.
//!
//! - `networking`: Implements networking-related features, enabling communication over the network, peer
//!    discovery, and protocol handling.
//!
//! - `protocol`: Represents shared protocol logic used for consistent communication and behavior across
//!    various components of the system.
//!
//! - `test_utils`: Provides utilities and helpers for testing the application and its components.
//!
//! - `types`: Defines common types and structures used throughout the application to ensure consistency
//!    and reusability.
//!
//! - `utils`: Contains general-purpose utility functions and tools that support the overall operation of
//!    the application.
//!
//! Note: The `dashboard` module is only compiled and included if the "dashboard" feature is enabled.
#[cfg(feature = "dashboard")]
pub mod dashboard;
pub mod blockchains;
pub mod config;
pub mod cryptography;
pub mod daemon;
pub mod networking;
pub mod transport;
pub mod protocol;
pub mod test_utils;
pub mod types;
pub mod utils;
