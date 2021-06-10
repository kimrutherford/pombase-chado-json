extern crate postgres;
extern crate getopts;

use postgres::{Connection, TlsMode};

use std::error::Error;
use std::env;
use std::process;

use getopts::Options;

extern crate pombase;

use pombase::db::*;
use pombase::web::config::*;
use pombase::web::data_build::*;
use pombase::interpro::parse_interpro;
use pombase::pfam::parse_pfam;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

fn main() -> Result<(), Box<dyn Error>> {
    print!("{} v{}\n", PKG_NAME, VERSION);

    let args: Vec<String> = env::args().collect();
    let mut opts = Options::new();

    opts.optflag("h", "help", "print this help message");
    opts.optopt("c", "config-file", "Configuration file name", "CONFIG");
    opts.optopt("C", "doc-config-file",
                "Documentation configuration file name", "DOC_CONFIG");
    opts.optopt("p", "postgresql-connection-string",
                "PostgresSQL connection string like: postgres://user:pass@host/db_name",
                "CONN_STR");
    opts.optopt("i", "domain-data-file",
                "The name of the InterPro data file generated by 'pombase-domain-process'",
                "FILE");
    opts.optopt("", "pfam-data-file",
                "The name of the Pfam data file",
                "FILE");
    opts.optopt("r", "rnacentral-data-file",
                "The name of the Rfam data file generated by 'pombase-rnacentral-process'",
                "FILE");
    opts.optopt("", "go-eco-mapping",
                "GO evidence code to ECO ID mapping from http://purl.obolibrary.org/obo/eco/gaf-eco-mapping.txt", "FILE");
    opts.optopt("d", "output-directory",
                "Destination directory for the output", "DIR");
    opts.optflag("j", "store-json",
                 "optionally create a 'web_json' schema to store the generated JSON in the database");

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
    if !matches.opt_present("doc-config-file") {
        print!("no --doc-config-file option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("postgresql-connection-string") {
        print!("no -p|--postgresql-connection-string option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("go-eco-mapping") {
        print!("no --go-eco-mapping option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("domain-data-file") {
        print!("no -i|--domain-data-file option\n");
        print_usage(&program, opts);
        process::exit(1);
    }
    if !matches.opt_present("output-directory") {
        print!("no -d|--output-directory option\n");
        print_usage(&program, opts);
        process::exit(1);
    }

    let config = Config::read(&matches.opt_str("c").unwrap());
    let doc_config = DocConfig::read(&matches.opt_str("C").unwrap());
    let connection_string = matches.opt_str("p").unwrap();
    let maybe_pfam_json = matches.opt_str("pfam-data-file");
    let interpro_json = matches.opt_str("i").unwrap();
    let maybe_rnacentral_json = matches.opt_str("r");
    let go_eco_mapping = GoEcoMapping::read(&matches.opt_str("go-eco-mapping").unwrap())?;
    let output_dir = matches.opt_str("d").unwrap();

    let conn = match Connection::connect(connection_string.as_str(), TlsMode::None) {
        Ok(conn) => conn,
        Err(err) => panic!("failed to connect using: {}, err: {}", connection_string, err)
    };

    let raw = Raw::new(&conn);
    let interpro_data = parse_interpro(&config, &interpro_json);
    let pfam_data =
        if let Some(pfam_json) = maybe_pfam_json {
            Some(parse_pfam(&pfam_json))
        } else {
            None
        };
    let rnacentral_data =
        if let Some(rnacentral_json) = maybe_rnacentral_json {
            Some(pombase::rnacentral::parse_annotation_json(&rnacentral_json)?)
        } else {
            None
        };
    let web_data_build = WebDataBuild::new(&raw, &interpro_data, &pfam_data,
                                           &rnacentral_data, &config);
    let web_data = web_data_build.get_web_data();

    match web_data.write(&config, &go_eco_mapping, &doc_config, &output_dir) {
        Ok(_) => (),
        Err(e) => {
            panic!("error while writing: {}", e);
        },
    }

    if matches.opt_present("store-json") {
        conn.execute("DROP SCHEMA IF EXISTS web_json CASCADE", &[])?;
        conn.execute("CREATE SCHEMA web_json", &[])?;
        conn.execute("CREATE EXTENSION IF NOT EXISTS pg_trgm;", &[])?;
        conn.execute("CREATE TABLE web_json.gene (uniquename TEXT, data JSONB)", &[])?;
        conn.execute("CREATE INDEX gene_uniquename_idx ON web_json.gene(uniquename)", &[])?;
        conn.execute("CREATE TABLE web_json.term (termid TEXT, data JSONB)", &[])?;
        conn.execute("CREATE INDEX term_termid_idx ON web_json.term(termid)", &[])?;
        conn.execute("CREATE TABLE web_json.reference (uniquename TEXT, data JSONB)", &[])?;
        conn.execute("CREATE INDEX reference_uniquename_idx on web_json.reference(uniquename)", &[])?;

        web_data.store_jsonb(&conn);

        print!("stored results as JSONB using {}\n", &connection_string);
    }

    Ok(())
}
