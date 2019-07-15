use crate::app_state::AppState;
use crate::datasource::*;
use crate::datasource_database::{SourceDatabase, SourceDatabaseParameters};
use crate::form_parameters::FormParameters;
use crate::pagelist::PageList;
use regex::Regex;
//use rayon::prelude::*;
use rocket::http::ContentType;
use rocket::http::Status;
use rocket::request::State;
use rocket::response::Responder;
use rocket::Request;
use rocket::Response;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

pub struct MyResponse {
    pub s: String,
    pub content_type: ContentType,
}

impl Responder<'static> for MyResponse {
    fn respond_to(self, _: &Request) -> Result<Response<'static>, Status> {
        Response::build()
            .header(self.content_type)
            .sized_body(Cursor::new(self.s))
            .ok()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Combination {
    None,
    Source(String),
    Intersection((Box<Combination>, Box<Combination>)),
    Union((Box<Combination>, Box<Combination>)),
    Not((Box<Combination>, Box<Combination>)),
}

#[derive(Debug, Clone)]
pub struct Platform {
    form_parameters: Arc<FormParameters>,
    pub state: Arc<AppState>,
    result: Option<PageList>,
}

impl Platform {
    pub fn new_from_parameters(form_parameters: &FormParameters, state: State<AppState>) -> Self {
        Self {
            form_parameters: Arc::new((*form_parameters).clone()),
            state: Arc::new(state.inner().clone()),
            result: None,
        }
    }

    pub fn run(&mut self) {
        // TODO legacy parameters

        let mut candidate_sources: Vec<Box<dyn DataSource>> = vec![];
        candidate_sources.push(Box::new(SourceDatabase::new(self.db_params())));
        candidate_sources.push(Box::new(SourceSparql::new()));
        candidate_sources.push(Box::new(SourceManual::new()));
        candidate_sources.push(Box::new(SourcePagePile::new()));
        candidate_sources.push(Box::new(SourceSearch::new()));
        candidate_sources.push(Box::new(SourceWikidata::new()));

        if !candidate_sources.iter().any(|source| source.can_run(&self)) {
            candidate_sources = vec![];
            candidate_sources.push(Box::new(SourceLabels::new()));
            if !candidate_sources.iter().any(|source| source.can_run(&self)) {
                return;
            }
        }

        let mut results: HashMap<String, Option<PageList>> = HashMap::new();
        // TODO threads

        for source in &mut candidate_sources {
            if source.can_run(&self) {
                results.insert(source.name(), source.run(&self));
            }
        }

        let available_sources = candidate_sources
            .iter()
            .filter(|s| s.can_run(&self))
            .map(|s| s.name())
            .collect();
        let combination = self.get_combination(available_sources);

        println!("{:#?}", &combination);

        self.result = self.combine_results(&mut results, &combination);
    }

    pub fn db_params(&self) -> SourceDatabaseParameters {
        /*
        // TODO Legacy parameters
        if ( params.find("comb_subset") != params.end() ) params["combination"] = "subset" ;
        if ( params.find("comb_union") != params.end() ) params["combination"] = "union" ;
        if ( params.find("get_q") != params.end() ) params["wikidata_item"] = "any" ;
        if ( params.find("wikidata") != params.end() ) params["wikidata_item"] = "any" ;
        if ( params.find("wikidata_no_item") != params.end() ) params["wikidata_item"] = "without" ;
        */

        let depth: u16 = self
            .get_param("depth")
            .unwrap_or("0".to_string())
            .parse::<u16>()
            .unwrap_or(0);
        let ret = SourceDatabaseParameters {
            combine: match self.form_parameters.params.get("combination") {
                Some(x) => {
                    if x == "union" {
                        x.to_string()
                    } else {
                        "subset".to_string()
                    }
                }
                _ => "subset".to_string(),
            },
            only_new_since: self.has_param("only_new_since"),
            max_age: self
                .get_param("max_age")
                .map(|x| x.parse::<i64>().unwrap_or(0)),
            before: self.get_param_blank("before"),
            after: self.get_param_blank("after"),
            templates_yes: self.get_param_as_vec("templates_yes", "\n"),
            templates_any: self.get_param_as_vec("templates_any", "\n"),
            templates_no: self.get_param_as_vec("templates_no", "\n"),
            templates_yes_talk_page: self.has_param("templates_use_talk_yes"),
            templates_any_talk_page: self.has_param("templates_use_talk_any"),
            templates_no_talk_page: self.has_param("templates_use_talk_no"),
            linked_from_all: self.get_param_as_vec("outlinks_yes", "\n"),
            linked_from_any: self.get_param_as_vec("outlinks_any", "\n"),
            linked_from_none: self.get_param_as_vec("outlinks_no", "\n"),
            links_to_all: self.get_param_as_vec("links_to_all", "\n"),
            links_to_any: self.get_param_as_vec("links_to_any", "\n"),
            links_to_none: self.get_param_as_vec("links_to_no", "\n"),
            last_edit_bot: self.get_param_default("edits[bots]", "both"),
            last_edit_anon: self.get_param_default("edits[anons]", "both"),
            last_edit_flagged: self.get_param_default("edits[flagged]", "both"),
            gather_link_count: self.has_param("minlinks") || self.has_param("maxlinks"),
            page_image: self.get_param_default("page_image", "any"),
            page_wikidata_item: self.get_param_default("wikidata_item", "any"),
            ores_type: self.get_param_blank("ores_type"),
            ores_prediction: self.get_param_default("ores_prediction", "any"),
            depth: depth,
            cat_pos: self.get_param_as_vec("categories", "\n"),
            cat_neg: self.get_param_as_vec("negcats", "\n"),
            ores_prob_from: self
                .get_param("ores_prob_from")
                .map(|x| x.parse::<f32>().unwrap_or(0.0)),
            ores_prob_to: self
                .get_param("ores_prob_to")
                .map(|x| x.parse::<f32>().unwrap_or(1.0)),
            redirects: self.get_param_blank("show_redirects"),
            minlinks: self
                .get_param("minlinks")
                .map(|i| i.parse::<usize>().unwrap()),
            maxlinks: self
                .get_param("maxlinks")
                .map(|i| i.parse::<usize>().unwrap()),
            larger: self
                .get_param("larger")
                .map(|i| i.parse::<usize>().unwrap()),
            smaller: self
                .get_param("smaller")
                .map(|i| i.parse::<usize>().unwrap()),
            wiki: self.get_main_wiki(),
            namespace_ids: self
                .form_parameters
                .ns
                .iter()
                .cloned()
                .collect::<Vec<usize>>(),
        };
        ret
    }

    pub fn get_main_wiki(&self) -> Option<String> {
        // TODO
        let language = self.get_param("language")?;
        let project = self.get_param("project")?;
        if project == "wikipedia" {
            Some(language.to_owned() + "wiki")
        } else {
            None
        }
    }

    pub fn get_response(&self) -> MyResponse {
        MyResponse {
            s: format!("{:#?}", self.result()),
            content_type: ContentType::Plain,
        }
    }

    pub fn get_param_as_vec(&self, param: &str, separator: &str) -> Vec<String> {
        match self.get_param(param) {
            Some(s) => s
                .split(separator)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            None => vec![],
        }
    }

    pub fn get_param_blank(&self, param: &str) -> String {
        self.get_param(param).unwrap_or("".to_string())
    }

    pub fn get_param_default(&self, param: &str, default: &str) -> String {
        let ret = self.get_param(param).unwrap_or(default.to_string());
        if ret.is_empty() {
            default.to_string()
        } else {
            ret
        }
    }

    pub fn append_sql(sql: &mut SQLtuple, sub: &mut SQLtuple) {
        sql.0 += &sub.0;
        sql.1.append(&mut sub.1);
    }

    /// Returns a tuple with a string containing comma-separated question marks, and the (non-empty) Vec elements
    pub fn prep_quote(strings: &Vec<String>) -> SQLtuple {
        let escaped: Vec<String> = strings
            .iter()
            .filter_map(|s| match s.trim() {
                "" => None,
                other => Some(other.to_string()),
            })
            .collect();
        let mut questionmarks: Vec<String> = Vec::new();
        questionmarks.resize(escaped.len(), "?".to_string());
        (questionmarks.join(","), escaped)
    }

    pub fn sql_tuple() -> SQLtuple {
        ("".to_string(), vec![])
    }

    fn get_label_sql_helper(&self, ret: &mut SQLtuple, part1: &str, part2: &str) {
        let mut types = vec![];
        if self.has_param(&("cb_labels_".to_owned() + part1 + "_l")) {
            types.push("label");
        }
        if self.has_param(&("cb_labels_".to_owned() + part1 + "_a")) {
            types.push("alias");
        }
        if self.has_param(&("cb_labels_".to_owned() + part1 + "_d")) {
            types.push("description");
        }
        if !types.is_empty() {
            let mut tmp = Self::prep_quote(&types.iter().map(|s| s.to_string()).collect());
            ret.0 += &(" AND ".to_owned() + part2 + &" IN (".to_owned() + &tmp.0 + ")");
            ret.1.append(&mut tmp.1);
        }
    }

    pub fn get_label_sql(&self) -> SQLtuple {
        lazy_static! {
            static ref RE1: Regex = Regex::new(r#"[^a-z,]"#).unwrap();
        }
        let mut ret: SQLtuple = ("".to_string(), vec![]);
        let yes = self.get_param_as_vec("labels_yes", "\n");
        let any = self.get_param_as_vec("labels_any", "\n");
        let no = self.get_param_as_vec("labels_no", "\n");
        if yes.len() + any.len() + no.len() == 0 {
            return ret;
        }

        let langs_yes = self.get_param_as_vec("langs_labels_yes", ",");
        let langs_any = self.get_param_as_vec("langs_labels_any", ",");
        let langs_no = self.get_param_as_vec("langs_labels_no", ",");

        ret.0 =
            "SELECT DISTINCT term_full_entity_id FROM wb_terms t1 WHERE term_entity_type='item'"
                .to_string();
        let field = "term_text".to_string(); // term_search_key case-sensitive; term_text case-insensitive?

        yes.iter().for_each(|s| {
            ret.0 += &(" AND ".to_owned() + &field + " LIKE ?");
            ret.1.push(s.to_string());
            if !langs_yes.is_empty() {
                let mut tmp = Self::prep_quote(&langs_yes);
                ret.0 += &(" AND term_language IN (".to_owned() + &tmp.0 + ")");
                ret.1.append(&mut tmp.1);
                self.get_label_sql_helper(&mut ret, "yes", "term_type");
            }
        });

        if !langs_any.is_empty() {
            ret.0 += " AND (";
            let mut first = true;
            yes.iter().for_each(|s| {
                if first {
                    first = false;
                } else {
                    ret.0 += " OR "
                }
                ret.0 += &(" ( ".to_owned() + &field + " LIKE ?");
                ret.1.push(s.to_string());
                if !langs_any.is_empty() {
                    let mut tmp = Self::prep_quote(&langs_any);
                    ret.0 += &(" AND term_language IN (".to_owned() + &tmp.0 + ")");
                    ret.1.append(&mut tmp.1);
                    self.get_label_sql_helper(&mut ret, "any", "term_type");
                }
                ret.0 += ")";
            });
            ret.0 += ")";
        }

        no.iter().for_each(|s| {
            ret.0 += " AND NOT EXISTS (SELECT t2.term_full_entity_id FROM wb_terms t2 WHERE";
            ret.0 +=
                " t2.term_full_entity_id=t1.term_full_entity_id AND t2.term_entity_type='item'";
            ret.0 += &(" AND t2.".to_owned() + &field + " LIKE ?");
            ret.1.push(s.to_string());
            if !langs_no.is_empty() {
                let mut tmp = Self::prep_quote(&langs_no);
                ret.0 += &(" AND t2.term_language IN (".to_owned() + &tmp.0 + ")");
                ret.1.append(&mut tmp.1);
                self.get_label_sql_helper(&mut ret, "no", "t2.term_type");
            }
            ret.0 += ")";
        });
        ret
    }

    pub fn just_to_suppress_warnings() {
        let _x =
            Combination::Intersection((Box::new(Combination::None), Box::new(Combination::None)));
        let _y = Combination::Not((Box::new(Combination::None), Box::new(Combination::None)));
    }

    fn parse_combination_string(&self, _s: &String) -> Combination {
        // TODO
        Combination::Source("".to_string())
    }

    /// Checks is the parameter is set, and non-blank
    pub fn has_param(&self, param: &str) -> bool {
        match self.form_parameters().params.get(&param.to_string()) {
            Some(s) => s != "",
            None => false,
        }
    }

    pub fn get_param(&self, param: &str) -> Option<String> {
        if self.has_param(param) {
            self.form_parameters()
                .params
                .get(&param.to_string())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    fn get_combination(&self, available_sources: Vec<String>) -> Combination {
        match self.get_param("source_combination") {
            Some(combination_string) => self.parse_combination_string(&combination_string),
            None => {
                let mut comb = Combination::None;
                for source in &available_sources {
                    if comb == Combination::None {
                        comb = Combination::Source(source.to_string());
                    } else {
                        comb = Combination::Union((
                            Box::new(Combination::Source(source.to_string())),
                            Box::new(comb),
                        ));
                    }
                }
                comb
            }
        }
    }

    fn combine_results(
        &self,
        results: &mut HashMap<String, Option<PageList>>,
        combination: &Combination,
    ) -> Option<PageList> {
        match combination {
            Combination::Source(s) => match results.get(s) {
                Some(r) => r.to_owned(),
                None => None,
            },
            Combination::Union((a, b)) => match (a.as_ref(), b.as_ref()) {
                (Combination::None, c) => self.combine_results(results, c),
                (c, Combination::None) => self.combine_results(results, c),
                (c, d) => {
                    let mut r1 = self.combine_results(results, c).unwrap();
                    let r2 = self.combine_results(results, d);
                    r1.union(r2).ok()?;
                    Some(r1)
                }
            },
            Combination::Intersection((a, b)) => match (a.as_ref(), b.as_ref()) {
                (Combination::None, _c) => None,
                (_c, Combination::None) => None,
                (c, d) => {
                    let mut r1 = self.combine_results(results, c).unwrap();
                    let r2 = self.combine_results(results, d);
                    r1.intersection(r2).ok()?;
                    Some(r1)
                }
            },
            Combination::Not((a, b)) => match (a.as_ref(), b.as_ref()) {
                (Combination::None, _c) => None,
                (c, Combination::None) => self.combine_results(results, c),
                (c, d) => {
                    let mut r1 = self.combine_results(results, c).unwrap();
                    let r2 = self.combine_results(results, d);
                    r1.difference(r2).ok()?;
                    Some(r1)
                }
            },
            Combination::None => None,
        }
    }

    pub fn result(&self) -> &Option<PageList> {
        &self.result
    }

    pub fn form_parameters(&self) -> &Arc<FormParameters> {
        &self.form_parameters
    }
}
