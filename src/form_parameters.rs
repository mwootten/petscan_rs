use regex::Regex;
use rocket::data::Outcome as DataOutcome;
use rocket::data::{FromData, Transform, Transformed};
use rocket::http::uri::Uri;
use rocket::http::{Method, Status};
use rocket::request::{self, FromRequest};
use rocket::Outcome;
use rocket::{Data, Outcome::*, Request};
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use url::*;

static FORM_SIZE_LIMIT: u64 = 1024 * 1024 * 50; // 50MB

#[derive(Debug, Clone)]
pub struct FormParameters {
    pub params: HashMap<String, String>,
    pub ns: HashSet<usize>,
}

impl FormParameters {
    /// Extracts namespaces from parameter list
    fn ns_from_params(params: &HashMap<String, String>) -> HashSet<usize> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r#"^ns\[(\d+)\]$"#).unwrap();
        }
        let mut ns: HashSet<usize> = HashSet::new();
        params
            .iter()
            .filter(|(_k, v)| *v == "1")
            .for_each(|(k, v)| {
                if k == "ns" && v == "*" {
                    // Backwards compat
                    ns.insert(0);
                }
                for cap in RE.captures_iter(k) {
                    match cap[1].parse::<usize>() {
                        Ok(ns_num) => {
                            ns.insert(ns_num);
                        }
                        _ => {}
                    }
                }
            });
        ns
    }

    /// Parses a query string into a new object
    pub fn outcome_from_query(query: &str) -> Result<Self, String> {
        // TODO PSID IMPORTANT for parsing see https://api.rocket.rs/v0.4/rocket/request/struct.Request.html#method.uri
        let parsed_url = match Url::parse(&("https://127.0.0.1/?".to_string() + query)) {
            Ok(url) => url,
            Err(e) => return Err(format!("{:?}", &e)),
        };
        let params: HashMap<_, _> = parsed_url.query_pairs().into_owned().collect();
        let ns = Self::ns_from_params(&params);
        let mut ret = FormParameters {
            params: params,
            ns: ns,
        };
        ret.legacy_parameters();
        Ok(ret)
    }

    /// Amends a an object based on a previous one (used for PSID in main.rs)
    pub fn rebase(&mut self, base: &FormParameters) {
        base.params.iter().for_each(|(k, v)| {
            if self.params.contains_key(k) {
                if self.params.get(k).unwrap().is_empty() {
                    self.params.insert(k.to_string(), v.to_string());
                }
            } else {
                self.params.insert(k.to_string(), v.to_string());
            }
        });
        self.legacy_parameters();
        self.ns = Self::ns_from_params(&self.params);
    }

    pub fn to_string(&self) -> String {
        self.params
            .iter()
            .map(|(k, v)| {
                Uri::percent_encode(k).to_string() + "=" + &Uri::percent_encode(v).to_string()
            })
            .collect::<Vec<String>>()
            .join("&")
    }

    fn has_param(&self, key: &str) -> bool {
        self.params.contains_key(&key.to_string())
    }

    fn set_param(&mut self, key: &str, value: &str) {
        self.params.insert(key.to_string(), value.to_string());
    }

    fn fallback(&mut self, key_primary: &str, key_fallback: &str) {
        if !self.has_param(key_fallback) {
            return;
        }
        if !self.has_param(key_primary) || self.params.get(key_primary) == Some(&"".to_string()) {
            let value = self.params.get(key_fallback).unwrap().to_owned();
            self.set_param(key_primary, &value);
        }
    }

    fn legacy_parameters(&mut self) {
        self.fallback("language", "lang");
        self.fallback("categories", "cats");
        if self.has_param("ns") && self.ns.is_empty() {
            let value = self.params.get("ns").unwrap().to_owned();
            if value == "*" {
                self.ns.insert(0);
            }
        }
        if self.has_param("comb_subset") {
            self.set_param("combination", "subset");
        }
        if self.has_param("comb_union") {
            self.set_param("combination", "union");
        }
        if self.has_param("get_q") {
            self.set_param("wikidata_item", "any");
        }
        if self.has_param("wikidata") {
            self.set_param("wikidata_item", "any");
        }
        if self.has_param("wikidata_no_item") {
            self.set_param("wikidata_item", "without");
        }
    }
}

// GET
impl<'a, 'r> FromRequest<'a, 'r> for FormParameters {
    type Error = String;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Self, Self::Error> {
        match request.method() {
            // TODO Not sure if method check is really necessary
            Method::Get => {
                match request.uri().query() {
                    Some(query) => match FormParameters::outcome_from_query(query) {
                        Ok(fp) => Outcome::Success(fp),
                        Err(e) => Outcome::Failure((Status::BadRequest, format!("{}", &e))),
                    },
                    None => {
                        let mut ret = FormParameters {
                            params: HashMap::new(),
                            ns: HashSet::new(),
                        };
                        ret.params
                            .insert("show_main_page".to_string(), "1".to_string());
                        Outcome::Success(ret)
                        //Outcome::Failure((Status::BadRequest, "No query found".to_string()))
                    }
                }
            }
            _ => Outcome::Failure((Status::BadRequest, "Unsupported method".to_string())),
        }
    }
}

// POST
impl<'b> FromData<'b> for FormParameters {
    type Error = String;
    type Owned = String;
    type Borrowed = str;

    fn transform(_: &Request, data: Data) -> Transform<DataOutcome<Self::Owned, Self::Error>> {
        let mut stream = data.open().take(FORM_SIZE_LIMIT);
        let mut string = String::with_capacity((FORM_SIZE_LIMIT / 2) as usize);
        let outcome = match stream.read_to_string(&mut string) {
            Ok(_) => Success(string),
            Err(e) => Failure((Status::InternalServerError, format!("{:?}", e))),
        };

        // Returning `Borrowed` here means we get `Borrowed` in `from_data`.
        Transform::Borrowed(outcome)
    }

    fn from_data(_: &Request, outcome: Transformed<'b, Self>) -> DataOutcome<Self, Self::Error> {
        let query = outcome.borrowed()?;
        match FormParameters::outcome_from_query(query) {
            Ok(fp) => Outcome::Success(fp),
            Err(e) => Outcome::Failure((Status::BadRequest, format!("{}", &e))),
        }
    }
}
