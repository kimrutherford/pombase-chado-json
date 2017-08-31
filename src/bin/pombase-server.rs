#![feature(plugin)]
#![plugin(rocket_codegen)]

extern crate getopts;
extern crate rocket;
#[macro_use] extern crate rocket_contrib;

#[macro_use] extern crate serde_derive;

extern crate pombase;

use std::sync::Mutex;
use std::process;
use std::env;
use std::path::{Path, PathBuf};

use getopts::Options;

use rocket_contrib::{Json, Value};

use rocket::response::NamedFile;

use pombase::api::query::Query;
use pombase::api::result::QueryAPIResult;
use pombase::api::search::Search;
use pombase::api::query_exec::QueryExec;
use pombase::api::server_data::ServerData;
use pombase::web::data::{SolrTermSummary, GeneDetails, GenotypeDetails,
                         TermDetails, ReferenceDetails};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

struct StaticFileState {
    web_root_dir: String,
}

// try the path, then try path + ".json", then default to loading the Angular app
// from /index.html
#[get("/<path..>", rank=3)]
fn get_misc(path: PathBuf, state: rocket::State<Mutex<StaticFileState>>) -> Option<NamedFile> {
    let web_root_dir = &state.lock().expect("failed to lock").web_root_dir;
    let root_dir_path = Path::new("/").join(web_root_dir);
    let full_path = root_dir_path.join(path);
    if full_path.exists() {
        return NamedFile::open(full_path).ok();
    }

    let mut json_path_str = full_path.to_str().unwrap().to_owned();
    json_path_str += ".json";
    let json_path: PathBuf = json_path_str.into();

    if json_path.exists() {
        return NamedFile::open(json_path).ok();
    }

    NamedFile::open(root_dir_path.join("index.html")).ok()
}

#[get("/api/v1/dataset/latest/data/gene/<id>", rank=2)]
fn get_gene(id: String, state: rocket::State<Mutex<QueryExec>>) -> Option<Json<GeneDetails>> {
    let query_exec = state.lock().expect("failed to lock");
    if let Some(gene) = query_exec.get_server_data().get_gene_details(&id) {
        Some(Json(gene.clone()))
    } else {
        None
    }
}

#[get("/api/v1/dataset/latest/data/genotype/<id>", rank=2)]
fn get_genotype(id: String, state: rocket::State<Mutex<QueryExec>>) -> Option<Json<GenotypeDetails>> {
    let query_exec = state.lock().expect("failed to lock");
    if let Some(genotype) = query_exec.get_server_data().get_genotype_details(&id) {
        Some(Json(genotype.clone()))
    } else {
        None
    }
}

#[get("/api/v1/dataset/latest/data/term/<id>", rank=2)]
fn get_term(id: String, state: rocket::State<Mutex<QueryExec>>) -> Option<Json<TermDetails>> {
    let query_exec = state.lock().expect("failed to lock");
    if let Some(term) = query_exec.get_server_data().get_term_details(&id) {
        Some(Json(term.clone()))
    } else {
        None
    }
}

#[get("/api/v1/dataset/latest/data/reference/<id>", rank=2)]
fn get_reference(id: String, state: rocket::State<Mutex<QueryExec>>) -> Option<Json<ReferenceDetails>> {
    let query_exec = state.lock().expect("failed to lock");
    if let Some(reference) = query_exec.get_server_data().get_reference_details(&id) {
        Some(Json(reference.clone()))
    } else {
        None
    }
}

#[get("/", rank=1)]
fn get_index(state: rocket::State<Mutex<StaticFileState>>) -> Option<NamedFile> {
    let web_root_dir = &state.lock().expect("failed to lock").web_root_dir;
    let root_dir_path = Path::new("/").join(web_root_dir);
    NamedFile::open(root_dir_path.join("index.html")).ok()
}

#[post("/api/v1/dataset/latest/query", rank=1, data="<q>", format = "application/json")]
fn query_post(q: Json<Query>, state: rocket::State<Mutex<QueryExec>>)
              -> Option<Json<QueryAPIResult>>
{
    let query_exec = state.lock().expect("failed to lock");
    Some(Json(query_exec.exec(&q.into_inner())))
}

#[get ("/reload")]
fn reload(state: rocket::State<Mutex<QueryExec>>) {
    let mut query_exec = state.lock().expect("failed to lock");
    print!("reloading ...\n");
    query_exec.reload();
    print!("... done\n");
}

#[derive(Serialize, Debug)]
struct CompletionResponse {
    status: String,
    matches: Vec<SolrTermSummary>,
}

#[get ("/api/v1/dataset/latest/complete/<cv_name>/<q>", rank=1)]
fn complete(cv_name: String, q: String, state: rocket::State<Mutex<Search>>)
              -> Option<Json<CompletionResponse>>
{
    let search = state.lock().expect("failed to lock");
    let res = search.term_complete(&cv_name, &q);

    let completion_response =
        match res {
            Ok(matches) => {
                CompletionResponse {
                    status: "Ok".to_owned(),
                    matches: matches,
                }
            },
            Err(err) => {
                println!("{:?}", err);
                CompletionResponse {
                    status: "Error".to_owned(),
                    matches: vec![],
                }
            },
        };

    Some(Json(completion_response))
}

#[get ("/ping", rank=1)]
fn ping() -> Option<String> {
    Some(String::from("OK") + " " + PKG_NAME + " " + VERSION)
}

#[error(404)]
fn not_found() -> Json<Value> {
    Json(json!({
        "status": "error",
        "reason": "Resource was not found."
    }))
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() {
    print!("{} v{}\n", PKG_NAME, VERSION);

    let args: Vec<String> = env::args().collect();
    let mut opts = Options::new();

    opts.optflag("h", "help", "print this help message");
    opts.optopt("c", "config-file", "Configuration file name", "CONFIG");
    opts.optopt("m", "search-maps", "Search data", "MAPS_JSON_FILE");
    opts.optopt("s", "gene-subsets", "Gene subset data", "SUBSETS_JSON_FILE");
    opts.optopt("w", "web-root-dir", "Root web data directory", "WEB_ROOT_DIR");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => panic!("Invalid options\n{}", f)
    };

    let program = args[0].clone();

    if matches.opt_present("help") {
        print_usage(&program, opts);
        process::exit(0);
    }

    if !matches.opt_present("config-file") {
        print!("no -c|--config-file option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("search-maps") {
        print!("no --search-maps|-m option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("gene-subsets") {
        print!("no --gene-subsets|-s option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("web-root-dir") {
        print!("no --web-root-dir|-w option\n");
        print_usage(&program, opts);
        process::exit(1);
    }

    let search_maps_filename = matches.opt_str("m").unwrap();
    let gene_subsets_filename = matches.opt_str("s").unwrap();
    println!("Reading data files ...");

    let config_file_name = matches.opt_str("c").unwrap();
    let server_data = ServerData::new(&config_file_name, &search_maps_filename,
                                      &gene_subsets_filename);
    let query_exec = QueryExec::new(server_data);
    let searcher = Search::new("http://localhost:8983/solr".to_owned());

    let web_root_dir = matches.opt_str("w").unwrap();
    let static_file_state = StaticFileState {
        web_root_dir: web_root_dir,
    };

    println!("Starting server ...");
    rocket::ignite()
        .mount("/", routes![get_index, get_misc, query_post,
                            get_gene, get_genotype, get_term, get_reference,
                            reload, complete, ping])
        .catch(errors![not_found])
        .manage(Mutex::new(query_exec))
        .manage(Mutex::new(searcher))
        .manage(Mutex::new(static_file_state))
        .launch();
}
