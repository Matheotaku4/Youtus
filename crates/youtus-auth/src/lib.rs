//! Authentification de Youtus : device flow OAuth, stockage du refresh token,
//! import de cookies pour les capacités que le client TV ne couvre pas.

pub mod cookie;
pub mod device;
pub mod token;

pub use cookie::{parse_netscape_cookies, Cookies};
pub use device::{poll_token, refresh, request_device_code, Credentials, DeviceCode, Error};
pub use token::{data_dir, default_token_path, load_token, save_token, StoredToken};
