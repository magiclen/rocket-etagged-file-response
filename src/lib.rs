//! # Etagged File Response for Rocket Framework
//! This crate provides a response struct used for offering static files with **Etag** cache.

extern crate mime_guess;
extern crate crc;
extern crate rocket_etag_if_none_match;
extern crate rocket;

use std::sync::Mutex;
use std::collections::HashMap;
use std::path::Path;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, BufReader};

use mime_guess::get_mime_type_str;

use crc::{crc64, Hasher64};

use rocket_etag_if_none_match::EtagIfNoneMatch;

use rocket::response::{self, Response, Responder};
use rocket::http::{Status, hyper::header::{ETag, EntityTag}};
use rocket::request::{Request, State};

const FILE_RESPONSE_CHUNK_SIZE: u64 = 4096;

/// This map should be managed by a rocket instance.
pub type EtagMap = Mutex<HashMap<String, String>>;

/// The response struct used for offering static files with **Etag** cache.
pub struct EtaggedFileResponse {
    pub data: Option<Box<Read>>,
    pub is_etag_match: bool,
    pub etag: String,
    pub content_type: Option<String>,
    pub content_length: Option<u64>,
}

impl<'a> Responder<'a> for EtaggedFileResponse {
    fn respond_to(self, _: &Request) -> response::Result<'a> {
        let mut response = Response::build();

        if self.is_etag_match {
            response.status(Status::NotModified);
        } else {
            response.header(ETag(EntityTag::new(true, self.etag.clone())));

            if let Some(content_type) = self.content_type {
                response.raw_header("Content-Type", content_type);
            }

            if let Some(content_length) = self.content_length {
                response.raw_header("Content-Length", content_length.to_string());
            }

            response.chunked_body(self.data.unwrap(), FILE_RESPONSE_CHUNK_SIZE);
        }

        response.ok()
    }
}

impl EtaggedFileResponse {
    /// Create a EtaggedFileResponse instance from a path of a file.
    pub fn from<P: AsRef<Path>>(etag_map: State<EtagMap>, etag_if_none_match: EtagIfNoneMatch, path: P) -> io::Result<EtaggedFileResponse> {
        let path = match path.as_ref().canonicalize() {
            Ok(path) => path,
            Err(e) => Err(e)?
        };

        if !path.is_file() {
            return Err(io::Error::from(ErrorKind::InvalidInput));
        }

        let path_str = path.to_str().unwrap();

        let etag = etag_map.lock().unwrap().get(path_str).map(|etag| { etag.clone() });

        let etag = match etag {
            Some(etag) => etag,
            None => {
                let mut digest = crc64::Digest::new(crc64::ECMA);

                let mut buffer = [0u8; FILE_RESPONSE_CHUNK_SIZE as usize];

                let read = File::open(&path)?;

                let mut reader = BufReader::new(read);

                loop {
                    match reader.read(&mut buffer) {
                        Ok(c) => {
                            if c == 0 {
                                break;
                            }
                            digest.write(&buffer[0..c]);
                        }
                        Err(error) => {
                            return Err(error);
                        }
                    }
                }

                let crc64 = digest.sum64();

                let etag = format!("{:X}", crc64);

                let path_string = path_str.to_string();

                let cloned_etag = etag.clone();

                etag_map.lock().unwrap().insert(path_string, cloned_etag);

                etag
            }
        };

        let is_etag_match = match etag_if_none_match.etag {
            Some(r_etag) => r_etag.tag().eq(&etag),
            None => false
        };

        if is_etag_match {
            Ok(EtaggedFileResponse {
                data: None,
                is_etag_match: true,
                etag,
                content_type: None,
                content_length: None,
            })
        } else {
            let file_size = match fs::metadata(&path) {
                Ok(metadata) => {
                    Some(metadata.len())
                }
                Err(e) => return Err(e)
            };

            let content_type = match path.extension() {
                Some(extension) => {
                    get_mime_type_str(&extension.to_str().unwrap().to_lowercase()).map(|t| { String::from(t) })
                }
                None => None
            };

            let data = Box::from(File::open(&path)?);

            Ok(EtaggedFileResponse {
                data: Some(data),
                is_etag_match: false,
                etag,
                content_type,
                content_length: file_size,
            })
        }
    }

    /// Create a new EtagMap instance.
    pub fn new_etag_map() -> EtagMap {
        Mutex::from(HashMap::<String, String>::new())
    }
}