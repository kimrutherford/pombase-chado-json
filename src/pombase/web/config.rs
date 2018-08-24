use std::collections::{HashMap, HashSet};
use std::io::BufReader;
use std::fs::File;

use types::*;
use serde_json;

use pombase_rc_string::RcString;

// configuration for extension display names and for the "Target of" section
#[derive(Deserialize, Clone, Debug)]
pub struct ExtensionDisplayNames {
    pub rel_name: RcString, // name of extension relation
    pub display_name: RcString, // text to display
    pub if_descendant_of: Option<RcString>, // None if applies to any extension
    pub reciprocal_display: Option<RcString>, // None if reciprocal shouldn't be displayed
}

// "interesting parents" are those stored in the JSON in the TermShort structs
#[derive(Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct InterestingParent {
    pub termid: RcString,
    pub rel_name: RcString,
}

// the order of relations within an extension:
#[derive(Deserialize, Clone, Debug)]
pub struct RelationOrder {
    // put the relations in this order in the displayed extensions:
    pub relation_order: Vec<String>,
    // except for these reactions which should always come last:
    pub always_last: Vec<String>,
}


#[derive(Deserialize, Clone, Debug)]
pub struct AncestorFilterCategory {
    pub display_name: RcString,
    // this category matches these terms and their descendants
    pub ancestors: Vec<RcString>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct FilterConfig {
    pub filter_name: String,
    pub display_name: String,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub term_categories: Vec<AncestorFilterCategory>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub extension_categories: Vec<AncestorFilterCategory>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SplitByParentsConfig {
    pub termids: Vec<RcString>,
    pub display_name: RcString,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ChromosomeConfig {
    // string to use for this chromosome in a file name, eg. "chromosome_II"
    // or "mitochondrial_chromosome"
    pub export_file_id: String,
    // string to use within files, eg. "II" or "mitochondrial"
    pub export_id: String,
    // eg. "Chromosome II" or "Mitochondrial chromosome"
    pub long_display_name: String,
    // eg. "II" or "Mitochondrial"
    pub short_display_name: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CvConfig {
    pub feature_type: RcString,
    // filtering configured per CV
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub filters: Vec<FilterConfig>,
    // config for splitting cv annotation tables into sub-sections
    // based on ancestry
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub split_by_parents: Vec<SplitByParentsConfig>,
    // relations to not show in the summary
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub summary_relations_to_hide: Vec<RcString>,
    // relations where the range is a gene ID to display like:
    //   has substrate pom1, cdc1 involved in negative regulation of ...
    // rather than as two lines
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub summary_relation_ranges_to_collect: Vec<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub sort_details_by: Option<Vec<RcString>>,
}

pub type ShortEvidenceCode = RcString;
pub type LongEvidenceCode = RcString;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfigOrganism {
    pub taxonid: OrganismTaxonId,
    pub genus: String,
    pub species: String,
}

impl ConfigOrganism {
    pub fn full_name(&self) -> String {
        self.genus.clone() + "_" + self.species.as_str()
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct ViabilityTerms {
    pub viable: RcString,
    pub inviable: RcString,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TermAndName {
    pub termid: RcString,
    pub name: RcString,
}

#[derive(Deserialize, Clone, Debug)]
pub struct InterPro {
    pub dbnames_to_filter: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ServerSubsetConfig {
    pub prefixes_to_remove: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ServerConfig {
    pub subsets: ServerSubsetConfig,
    pub solr_url: String,
    pub close_synonym_boost: f32,
    pub distant_synonym_boost: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct EvidenceDetails {
    pub long: LongEvidenceCode,
    pub link: Option<RcString>,
}

pub type DatabaseName = String;
pub type DatabaseAliases = HashMap<DatabaseName, DatabaseName>;

#[derive(Deserialize, Clone, Debug)]
pub struct QueryDataConfig {
    pub go_components: Vec<RcString>,
    pub go_process_superslim: Vec<RcString>,
    pub go_function: Vec<RcString>,
    pub ortholog_presence_taxonids: HashSet<u32>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct MacromolecularComplexesConfig {
    pub parent_complex_termid: String,
    pub excluded_terms: HashSet<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct FileExportConfig {
    pub macromolecular_complexes: Option<MacromolecularComplexesConfig>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    pub database_name: String,
    pub load_organism_taxonid: OrganismTaxonId,
    pub organisms: Vec<ConfigOrganism>,
    pub api_seq_chunk_sizes: Vec<usize>,
    pub extension_display_names: Vec<ExtensionDisplayNames>,
    pub extension_relation_order: RelationOrder,
    pub evidence_types: HashMap<ShortEvidenceCode, EvidenceDetails>,
    pub cv_config: HashMap<CvName, CvConfig>,
// when creating a TermShort struct, for each of these termids if the term has
// an "interesting parent" using the given rel_name, we store it in the
// interesting_parents field of the TermShort
    pub interesting_parents: Vec<InterestingParent>,
    pub viability_terms: ViabilityTerms,
    pub go_slim_terms: Vec<TermAndName>,
    pub interpro: InterPro,
    pub server: ServerConfig,
    pub extra_database_aliases: DatabaseAliases,
    pub chromosomes: HashMap<String, ChromosomeConfig>,
    pub query_data_config: QueryDataConfig,
    pub file_exports: FileExportConfig,
}

impl Config {

    pub fn read(config_file_name: &str) -> Config {
        let file = match File::open(config_file_name) {
            Ok(file) => file,
            Err(err) => {
                panic!("Failed to read {}: {}\n", config_file_name, err)
            }
        };
        let reader = BufReader::new(file);

        match serde_json::from_reader(reader) {
            Ok(config) => config,
            Err(err) => {
                panic!("failed to parse {}: {}", config_file_name, err)
            },
        }
    }

    pub fn cv_config_by_name(&self, cv_name: &str) -> CvConfig {
        if let Some(config) = self.cv_config.get(cv_name) {
            config.clone()
        } else {
            if cv_name.starts_with("extension:") {
                if cv_name.ends_with(":gene") {
                    CvConfig {
                        feature_type: "gene".into(),
                        filters: vec![],
                        split_by_parents: vec![],
                        summary_relations_to_hide: vec![],
                        summary_relation_ranges_to_collect: vec![],
                        sort_details_by: None,
                    }
                } else {
                    CvConfig {
                        feature_type: "genotype".into(),
                        filters: vec![],
                        split_by_parents: vec![],
                        summary_relations_to_hide: vec![],
                        summary_relation_ranges_to_collect: vec![],
                        sort_details_by: None,
                    }
                }
            } else {
                CvConfig {
                    feature_type: "gene".into(),
                    filters: vec![],
                    split_by_parents: vec![],
                    summary_relations_to_hide: vec![],
                    summary_relation_ranges_to_collect: vec![],
                    sort_details_by: None,
                }
            }
        }
    }

    pub fn load_organism(&self) -> ConfigOrganism {
        for org in &self.organisms {
            if org.taxonid == self.load_organism_taxonid {
                return org.clone();
            }
        }

        panic!("can't find configuration for load_organism_taxonid: {}",
               self.load_organism_taxonid);
    }

    pub fn find_chromosome_config<'a>(&'a self, chromosome_name: &str)
                                      -> &'a ChromosomeConfig
    {
        if let Some(ref chr_conf) = self.chromosomes.get(chromosome_name) {
            &chr_conf
        } else {
            panic!("can't find chromosome configuration for {}", &chromosome_name);
        }
    }
}

pub const POMBASE_ANN_EXT_TERM_CV_NAME: &str = "PomBase annotation extension terms";
pub const ANNOTATION_EXT_REL_PREFIX: &str = "annotation_extension_relation-";

pub const DB_NAME: &str = "PomBase";

pub enum FeatureRelAnnotationType {
    Interaction,
    Ortholog,
    Paralog,
}
pub struct FeatureRelConfig {
    pub rel_type_name: &'static str,
    pub annotation_type: FeatureRelAnnotationType,
}
pub const FEATURE_REL_CONFIGS: [FeatureRelConfig; 4] =
    [
        FeatureRelConfig {
            rel_type_name: "interacts_physically",
            annotation_type: FeatureRelAnnotationType::Interaction,
        },
        FeatureRelConfig {
            rel_type_name: "interacts_genetically",
            annotation_type: FeatureRelAnnotationType::Interaction,
        },
        FeatureRelConfig {
            rel_type_name: "orthologous_to",
            annotation_type: FeatureRelAnnotationType::Ortholog,
        },
        FeatureRelConfig {
            rel_type_name: "paralogous_to",
            annotation_type: FeatureRelAnnotationType::Paralog,
        },
    ];

// relations to use when copy annotation to parents (ie. adding the
// annotation of child terms to parents)
pub const DESCENDANT_REL_NAMES: [&str; 7] =
    ["is_a", "part_of", "regulates", "positively_regulates", "negatively_regulates",
     "has_part", "output_of"];
// only consider has_part relations for these ontologies:
pub const HAS_PART_CV_NAMES: [&str; 1] = ["fission_yeast_phenotype"];

// number of genes before (and after) to add to the gene_neighbourhood field
pub const GENE_NEIGHBOURHOOD_DISTANCE: usize = 5;

pub const TRANSCRIPT_FEATURE_TYPES: [&str; 7] =
    ["snRNA", "rRNA", "mRNA", "snoRNA", "ncRNA", "tRNA", "pseudogenic_transcript"];
pub const TRANSCRIPT_PART_TYPES: [&str; 4] =
    ["five_prime_UTR", "exon", "pseudogenic_exon", "three_prime_UTR"];
// any feature with a type not in this list or in the two TRANSCRIPT lists above
// will be stored in the other_features map
pub const HANDLED_FEATURE_TYPES: [&str; 7] =
    ["gene", "pseudogene", "intron", "genotype", "allele", "chromosome", "polypeptide"];

