extern crate serde_json;
extern crate postgres;

use std::cmp::min;
use std::fs::{File, create_dir_all};
use std::io::{Write, BufWriter};
use std::io;
use std::collections::BTreeMap;
use std::fmt::Display;
use std::fmt;
use std::collections::HashMap;
use regex::Regex;

use pombase_rc_string::RcString;

use crate::bio::util::{format_fasta, format_gene_gff, format_misc_feature_gff};

use flate2::Compression;
use flate2::write::GzEncoder;

use self::postgres::Connection;

type CvName = RcString;

pub type TypeInteractionAnnotationMap =
    HashMap<TypeName, Vec<InteractionAnnotation>>;
pub type UniquenameGeneMap =
    BTreeMap<GeneUniquename, GeneDetails>;
pub type UniquenameTranscriptMap =
    HashMap<TranscriptUniquename, TranscriptDetails>;
pub type UniquenameProteinMap =
    HashMap<ProteinUniquename, ProteinDetails>;
pub type UniquenameReferenceMap =
    HashMap<TermId, ReferenceDetails>;

pub type UniquenameAlleleMap = HashMap<AlleleUniquename, AlleleShort>;
pub type UniquenameGenotypeMap = HashMap<GenotypeUniquename, GenotypeDetails>;
pub type UniquenameFeatureShortMap = HashMap<RcString, FeatureShort>;
pub type TermIdDetailsMap = HashMap<TermId, TermDetails>;
pub type ChrNameDetailsMap = BTreeMap<ChromosomeName, ChromosomeDetails>;

pub type IdGenotypeMap = HashMap<GenotypeUniquename, GenotypeDetails>;
pub type IdGeneShortMap = HashMap<GeneUniquename, GeneShort>;
pub type IdRcTermShortMap = HashMap<TermId, Rc<TermShort>>;
pub type IdRcTermDetailsMap = HashMap<TermId, Rc<TermDetails>>;

pub type GeneShortMap = HashMap<GeneUniquename, GeneShort>;
pub type GenotypeShortMap = HashMap<GeneUniquename, GenotypeShort>;
pub type AlleleShortMap = HashMap<AlleleUniquename, AlleleShort>;
pub type TermShortMap = HashMap<TermId, TermShort>;

pub type OntAnnotationId = i32;
pub type IdOntAnnotationDetailMap = HashMap<OntAnnotationId, OntAnnotationDetail>;

pub type TermShortOptionMap = HashMap<TermId, Option<TermShort>>;
pub type GeneShortOptionMap = HashMap<GeneUniquename, Option<GeneShort>>;
pub type ReferenceShortOptionMap = HashMap<ReferenceUniquename, Option<ReferenceShort>>;

use std::rc::Rc;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::collections::HashSet;

use crate::web::config::*;
use crate::types::*;
use crate::rnacentral::*;
use crate::interpro::InterProMatch;

const FASTA_SEQ_COLUMNS: usize = 60;

fn write_as_fasta(writer: &mut dyn Write, id: &str, desc: Option<String>, seq: &str) {
    let fasta = format_fasta(id, desc, &seq, FASTA_SEQ_COLUMNS);
    writer.write_all(fasta.as_bytes()).unwrap();
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Throughput {
#[serde(rename = "high")]
    HighThroughput,
#[serde(rename = "low")]
    LowThroughput,
#[serde(rename = "non-experimental")]
    NonExperimental,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Hash)]
pub enum ExtRange {
#[serde(rename = "gene_uniquename")]
    Gene(GeneUniquename),
#[serde(rename = "promoter_gene_uniquename")]
    Promoter(RcString),
#[serde(rename = "summary_gene_uniquenames")]
    // the inner Vec length will be > 1 for cases like "binds abc1 and def2, cdc2"
    SummaryGenes(Vec<Vec<RcString>>),
#[serde(rename = "termid")]
    Term(TermId),
#[serde(rename = "summary_termids")]
    // See: merge_ext_part_ranges()
    SummaryTerms(Vec<TermId>),
#[serde(rename = "misc")]
    Misc(RcString),
#[serde(rename = "domain")]
    Domain(RcString),
#[serde(rename = "gene_product")]
    GeneProduct(TermId),  // eg.  PR:000027705
#[serde(rename = "summary_residues")]
    SummaryModifiedResidues(Vec<Residue>),
}

impl ExtRange {
    pub fn is_gene(&self) -> bool {
        match *self {
            ExtRange::Gene(_) => true,
            _ => false,
        }
    }
}

// A single part of an extension.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExtPart {
    pub rel_type_name: RcString,
    pub rel_type_display_name: RcString,
    pub ext_range: ExtRange,
}
impl PartialEq for ExtPart {
    fn eq(&self, other: &Self) -> bool {
        self.rel_type_name == other.rel_type_name &&
            self.ext_range == other.ext_range
    }
}
impl Eq for ExtPart {
}
impl Hash for ExtPart {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.rel_type_name.hash(state);
        self.ext_range.hash(state);
    }
}

// minimal information about a gene used in other objects
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneShort {
    pub uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<GeneName>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub product: Option<GeneProduct>,
}

impl GeneShort {
    pub fn from_gene_details(gene_details: &GeneDetails) -> Self {
        GeneShort {
            uniquename: gene_details.uniquename.clone(),
            name: gene_details.name.clone(),
            product: gene_details.product.clone(),
        }
    }

    pub fn display_name(&self) -> String {
        if let Some(ref name) = self.name {
            format!("{} ({})", name, self.uniquename)
        } else {
            String::from(&self.uniquename)
        }
    }
}

impl PartialEq for GeneShort {
    fn eq(&self, other: &GeneShort) -> bool {
        self.uniquename == other.uniquename
    }
}
impl Eq for GeneShort { }
impl Ord for GeneShort {
    fn cmp(&self, other: &GeneShort) -> Ordering {
        if self.name.is_some() {
            if other.name.is_some() {
                self.name.cmp(&other.name)
            } else { Ordering::Less }
        } else {
            if other.name.is_some() {
                Ordering::Greater
            } else { self.uniquename.cmp(&other.uniquename) }
        }
    }
}
impl PartialOrd for GeneShort {
    fn partial_cmp(&self, other: &GeneShort) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Hash for GeneShort {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.uniquename.hash(state);
    }
}

// a gene uniquename and an organism ID
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IdAndOrganism {
    pub identifier: RcString,
    pub taxonid: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IdNameAndOrganism {
    pub identifier: RcString,
    pub name: Option<RcString>,
    pub taxonid: u32,
}

// identifiers used for autocomplete in the search box
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneSummary {
    pub uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<GeneName>,
    pub taxonid: OrganismTaxonId,
    #[serde(skip_serializing_if="Option::is_none")]
    pub product: Option<GeneProduct>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub uniprot_identifier: Option<RcString>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub synonyms: Vec<RcString>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub orthologs: Vec<IdNameAndOrganism>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub location: Option<ChromosomeLocation>,
    pub feature_type: RcString,
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TermXref {
    pub xref_id: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub xref_display_name: Option<RcString>,
}

// minimal information about a terms used in other objects
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TermShort {
    pub name: TermName,
    pub cv_name: RcString,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub interesting_parents: HashSet<RcString>,
    pub termid: TermId,
    pub is_obsolete: bool,
    pub gene_count: usize,
    pub genotype_count: usize,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub xrefs: HashMap<RcString, TermXref>,
}

impl TermShort {
    pub fn from_term_details(term_details: &TermDetails) -> Self {
        TermShort {
            name: term_details.name.clone(),
            cv_name: term_details.cv_name.clone(),
            interesting_parents: term_details.interesting_parents.clone(),
            termid: term_details.termid.clone(),
            is_obsolete: term_details.is_obsolete,
            gene_count: term_details.gene_count,
            genotype_count: term_details.genotype_count,
            xrefs: term_details.xrefs.clone(),
        }
    }
}

impl PartialEq for TermShort {
    fn eq(&self, other: &TermShort) -> bool {
        self.termid == other.termid
    }
}
impl Eq for TermShort { }
impl Ord for TermShort {
    fn cmp(&self, other: &TermShort) -> Ordering {
        let order = self.name.cmp(&other.name);
        if order == Ordering::Equal {
            self.termid.cmp(&other.termid)
        } else {
            order
        }
    }
}
impl PartialOrd for TermShort {
    fn partial_cmp(&self, other: &TermShort) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Hash for TermShort {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.termid.hash(state);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChromosomeDetails {
    pub name: RcString,
    pub residues: RcString,
    pub ena_identifier: RcString,
    pub gene_uniquenames: Vec<RcString>,
    pub taxonid: OrganismTaxonId,
}

impl ChromosomeDetails {
    pub fn make_chromosome_short(&self) -> ChromosomeShort {
        ChromosomeShort {
            name: self.name.clone(),
            length: self.residues.len(),
            ena_identifier: self.ena_identifier.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReferenceShort {
    pub uniquename: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub title: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub citation: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub authors_abbrev: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub publication_year: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub approved_date: Option<RcString>,
    pub gene_count: usize,
    pub genotype_count: usize,
}

impl ReferenceShort {
    pub fn from_reference_details(reference_details: &ReferenceDetails) -> ReferenceShort {
        ReferenceShort {
            uniquename: reference_details.uniquename.clone(),
            title: reference_details.title.clone(),
            citation: reference_details.citation.clone(),
            publication_year: reference_details.publication_year.clone(),
            authors_abbrev: reference_details.authors_abbrev.clone(),
            approved_date: reference_details.approved_date.clone(),
            gene_count: reference_details.genes_by_uniquename.keys().len(),
            genotype_count: reference_details.genotypes_by_uniquename.keys().len(),
        }
    }
}

#[derive(PartialEq)]
pub enum ContainerType {
    Gene,
    Term,
    Reference,
    Genotype,
}

pub trait Container {
    fn container_type(&self) -> ContainerType;
}

pub trait AnnotationContainer: Container {
    fn cv_annotations(&self) -> &OntAnnotationMap;
    fn cv_annotations_mut(&mut self) -> &mut OntAnnotationMap;
    fn annotation_details(&self) -> &IdOntAnnotationDetailMap;
    fn terms_by_termid(&self) -> &TermShortOptionMap;
    fn genes_by_uniquename(&self) -> &GeneShortOptionMap;
    fn genotypes_by_uniquename(&self) -> Option<&HashMap<GenotypeUniquename, GenotypeShort>>;
}

pub trait OrthologAnnotationContainer: AnnotationContainer {
    fn ortholog_annotations(&self) -> &Vec<OrthologAnnotation>;
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReferenceDetails {
    pub uniquename: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub title: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub citation: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none", rename = "abstract")]
    pub pubmed_abstract: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub authors: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub authors_abbrev: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub pubmed_publication_date: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub publication_year: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_annotation_status: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_triage_status: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_curator_role: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_curator_name: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_first_approved_date: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_approved_date: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_session_submitted_date: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_added_date: Option<RcString>,

    // This is set to the year part of canto_first_approved_date if it is
    // not None, otherwise set to the year part of canto_approved_date, otherwise
    // canto_session_submitted_date
    #[serde(skip_serializing_if="Option::is_none")]
    pub approved_date: Option<RcString>,
    pub cv_annotations: OntAnnotationMap,
    pub physical_interactions: Vec<InteractionAnnotation>,
    pub genetic_interactions: Vec<InteractionAnnotation>,
    pub ortholog_annotations: Vec<OrthologAnnotation>,
    pub paralog_annotations: Vec<ParalogAnnotation>,
    pub genes_by_uniquename: GeneShortOptionMap,
    pub genotypes_by_uniquename: HashMap<GenotypeUniquename, GenotypeShort>,
    pub alleles_by_uniquename: HashMap<AlleleUniquename, AlleleShort>,
    pub terms_by_termid: TermShortOptionMap,
    pub annotation_details: IdOntAnnotationDetailMap,
}

impl Container for ReferenceDetails {
    fn container_type(&self) -> ContainerType {
        ContainerType::Reference
    }
}

impl AnnotationContainer for ReferenceDetails {
    fn cv_annotations(&self) -> &OntAnnotationMap {
        &self.cv_annotations
    }
    fn cv_annotations_mut(&mut self) -> &mut OntAnnotationMap {
        &mut self.cv_annotations
    }
    fn annotation_details(&self) -> &IdOntAnnotationDetailMap {
        &self.annotation_details
    }
    fn terms_by_termid(&self) -> &TermShortOptionMap {
        &self.terms_by_termid
    }
    fn genes_by_uniquename(&self) -> &GeneShortOptionMap {
        &self.genes_by_uniquename
    }
    fn genotypes_by_uniquename(&self) -> Option<&HashMap<GenotypeUniquename, GenotypeShort>> {
        Some(&self.genotypes_by_uniquename)
    }
}

impl OrthologAnnotationContainer for ReferenceDetails {
    fn ortholog_annotations(&self) -> &Vec<OrthologAnnotation> {
        &self.ortholog_annotations
    }
}

// the GO with/from
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WithFromValue {
    Gene(GeneShort),
    Term(TermShort),
    Identifier(RcString)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OntAnnotationDetail {
    pub id: i32,
    pub genes: Vec<GeneUniquename>,
    pub reference: Option<ReferenceUniquename>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub evidence: Option<Evidence>,
    pub extension: Vec<ExtPart>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub withs: HashSet<WithFromValue>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub froms: HashSet<WithFromValue>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub residue: Option<Residue>,
    pub qualifiers: Vec<Qualifier>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub gene_ex_props: Option<GeneExProps>,
    // only for genotype/phenotype annotation:
    #[serde(skip_serializing_if="Option::is_none")]
    pub genotype: Option<GenotypeUniquename>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub genotype_background: Option<RcString>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub conditions: HashSet<TermId>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub assigned_by: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub throughput: Option<Throughput>,
}

impl PartialEq for OntAnnotationDetail {
    fn eq(&self, other: &OntAnnotationDetail) -> bool {
        self.id == other.id
    }
}
impl Eq for OntAnnotationDetail { }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OntTermAnnotations {
    pub term: TermId,
    pub is_not: bool,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub rel_names: HashSet<RelName>,
    pub annotations: Vec<OntAnnotationId>,
    pub summary: Option<Vec<TermSummaryRow>>,
}

impl PartialEq for OntTermAnnotations {
    fn eq(&self, other: &OntTermAnnotations) -> bool {
        self.term == other.term
    }
}
impl Eq for OntTermAnnotations { }
impl Hash for OntTermAnnotations {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.term.hash(state);
        self.is_not.hash(state);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OntAnnotation {
    pub term_short: TermShort,
    pub id: i32,
    pub genes: HashSet<GeneShort>,
    pub reference_short: Option<ReferenceShort>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub evidence: Option<Evidence>,
    pub extension: Vec<ExtPart>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub withs: HashSet<WithFromValue>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub froms: HashSet<WithFromValue>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub residue: Option<Residue>,
    pub qualifiers: Vec<Qualifier>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub gene_ex_props: Option<GeneExProps>,
    // only for genotype/phenotype annotation:
    #[serde(skip_serializing_if="Option::is_none")]
    pub genotype_short: Option<GenotypeShort>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub genotype_background: Option<RcString>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub conditions: HashSet<TermShort>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub assigned_by: Option<RcString>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TermSummaryRow {
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub gene_uniquenames: Vec<GeneUniquename>, // for term and ref pages
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub genotype_uniquenames: Vec<GenotypeUniquename>, // for term pages
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub extension: Vec<ExtPart>,
}
impl PartialEq for TermSummaryRow {
    fn eq(&self, other: &TermSummaryRow) -> bool {
        self.gene_uniquenames == other.gene_uniquenames &&
            self.genotype_uniquenames == other.genotype_uniquenames &&
            self.extension == other.extension
    }
}
impl Hash for TermSummaryRow {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.gene_uniquenames.hash(state);
        self.genotype_uniquenames.hash(state);
        for ext_part in &self.extension {
            ext_part.hash(state);
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TargetOfAnnotation {
    pub ontology_name: RcString,
    pub ext_rel_display_name: RcString,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub genes: Vec<GeneUniquename>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub genotype_uniquename: Option<GenotypeUniquename>,
    pub reference_uniquename: Option<ReferenceUniquename>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SynonymDetails {
    pub name: RcString,
    #[serde(rename = "type")]
    pub synonym_type: RcString
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Strand {
    Forward = 1,
    Reverse = -1,
    Unstranded = 0,
}

impl Strand {
    pub fn to_gff_str(&self) -> &'static str {
        match *self {
            Strand::Forward => "+",
            Strand::Reverse => "-",
            Strand::Unstranded => ".",
        }
    }
}

impl Display for Strand {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.to_gff_str())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChromosomeShort {
    pub name: RcString,
    pub length: usize,
    pub ena_identifier: RcString,
}

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub enum Phase {
    Zero,
    One,
    Two,
}

impl Phase {
    pub fn to_gff_str(&self) -> &'static str {
        match *self {
            Phase::Zero => "0",
            Phase::One => "1",
            Phase::Two => "2",
        }
    }
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChromosomeLocation {
    pub chromosome_name: RcString,
    pub start_pos: usize,
    pub end_pos: usize,
    pub strand: Strand,
    #[serde(skip_serializing_if="Option::is_none")]
    pub phase: Option<Phase>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeletionViability {
    Viable,
    Inviable,
    DependsOnConditions,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PresentAbsent {
    Present,
    Absent,
    NotApplicable,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneDetails {
    pub uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<RcString>,
    pub taxonid: u32,
    #[serde(skip_serializing_if="Option::is_none")]
    pub product: Option<RcString>,
    pub deletion_viability: DeletionViability,
    #[serde(skip_serializing_if="Option::is_none")]
    pub uniprot_identifier: Option<RcString>,
    pub biogrid_interactor_id: Option<u32>,
    pub interpro_matches: Vec<InterProMatch>,
    // non-InterPro domains:
    pub tm_domain_coords: Vec<(usize, usize) >,
    pub orfeome_identifier: Option<RcString>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub name_descriptions: Vec<RcString>,
    pub synonyms: Vec<SynonymDetails>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub dbxrefs: HashSet<RcString>,
    pub feature_type: RcString,
    pub transcript_so_termid: TermId,
    #[serde(skip_serializing_if="Option::is_none")]
    pub characterisation_status: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub taxonomic_distribution: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub location: Option<ChromosomeLocation>,
    pub gene_neighbourhood: Vec<GeneShort>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub transcripts: Vec<TranscriptDetails>,
    pub cv_annotations: OntAnnotationMap,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub physical_interactions: Vec<InteractionAnnotation>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub genetic_interactions: Vec<InteractionAnnotation>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub ortholog_annotations: Vec<OrthologAnnotation>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub paralog_annotations: Vec<ParalogAnnotation>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub target_of_annotations: Vec<TargetOfAnnotation>,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub references_by_uniquename: ReferenceShortOptionMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub genes_by_uniquename: GeneShortOptionMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub genotypes_by_uniquename: HashMap<GenotypeUniquename, GenotypeShort>,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub alleles_by_uniquename: HashMap<AlleleUniquename, AlleleShort>,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub terms_by_termid: TermShortOptionMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub annotation_details: IdOntAnnotationDetailMap,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub feature_publications: HashSet<ReferenceUniquename>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    // A Vec of the term IDs of subsets for this gene.  Any useful subset
    // that contains any term for any annotation in the gene is included.
    // "useful" means that the front end might need it, eg. slim term IDs
    pub subset_termids: HashSet<TermId>,
}

impl GeneDetails {
    pub fn spliced_transcript_sequence(&self) -> Option<RcString> {
        if self.transcripts.len() > 1 {
            panic!("no support for multi-transcript genes");
        }

        if let Some(transcript) = self.transcripts.get(0) {
            let mut seq = String::new();

            for part in &transcript.parts {
                if part.feature_type == FeatureType::Exon {
                    seq += &part.residues;
                }
            }

            Some(RcString::from(&seq))
        } else {
            None
        }
    }
}

impl Container for GeneDetails {
    fn container_type(&self) -> ContainerType {
        ContainerType::Gene
    }
}

impl AnnotationContainer for GeneDetails {
    fn cv_annotations(&self) -> &OntAnnotationMap {
        &self.cv_annotations
    }
    fn cv_annotations_mut(&mut self) -> &mut OntAnnotationMap {
        &mut self.cv_annotations
    }
    fn annotation_details(&self) -> &IdOntAnnotationDetailMap {
        &self.annotation_details
    }
    fn terms_by_termid(&self) -> &TermShortOptionMap {
        &self.terms_by_termid
    }
    fn genes_by_uniquename(&self) -> &GeneShortOptionMap {
        &self.genes_by_uniquename
    }
    fn genotypes_by_uniquename(&self) -> Option<&HashMap<GenotypeUniquename, GenotypeShort>> {
        Some(&self.genotypes_by_uniquename)
    }
}

impl OrthologAnnotationContainer for GeneDetails {
    fn ortholog_annotations(&self) -> &Vec<OrthologAnnotation> {
        &self.ortholog_annotations
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProteinDetails {
    pub uniquename: TranscriptUniquename,
    pub sequence: RcString,
    pub molecular_weight: f32,
    pub average_residue_weight: f32,
    pub charge_at_ph7: f32,
    pub isoelectric_point: f32,
    pub codon_adaptation_index: f32,
}

pub type Residues = RcString;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum FeatureType {
#[serde(rename = "five_prime_utr")]
    FivePrimeUtr,
#[serde(rename = "five_prime_utr_intron")]
    FivePrimeUtrIntron,
#[serde(rename = "exon")]
    Exon,
#[serde(rename = "cds_intron")]
    // type for introns between exons
    CdsIntron,
#[serde(rename = "three_prime_utr")]
    ThreePrimeUtr,
#[serde(rename = "three_prime_utr_intron")]
    ThreePrimeUtrIntron,
#[serde(rename = "dg_repeat")]
    DGRepeat,
#[serde(rename = "dh_repeat")]
    DHRepeat,
#[serde(rename = "gap")]
    Gap,
#[serde(rename = "gene_group")]
    GeneGroup,
#[serde(rename = "long_terminal_repeat")]
    LongTerminalRepeat,
#[serde(rename = "low_complexity_region")]
    LowComplexityRegion,
#[serde(rename = "LTR_retrotransposon")]
    LTRRetrotransposon,
#[serde(rename = "mating_type_region")]
    MatingTypeRegion,
#[serde(rename = "nuclear_mt_pseudogene")]
    NuclearMtPseudogene,
#[serde(rename = "origin_of_replication")]
    OriginOfReplication,
#[serde(rename = "polyA_signal_sequence")]
    PolyASignalSequence,
#[serde(rename = "polyA_site")]
    PolyASite,
#[serde(rename = "promoter")]
    Promoter,
#[serde(rename = "region")]
    Region,
#[serde(rename = "regional_centromere")]
    RegionalCentromere,
#[serde(rename = "regional_centromere_central_core")]
    RegionalCentromereCentralCore,
#[serde(rename = "regional_centromere_inner_repeat_region")]
    RegionalCentromereInnerRepeatRegion,
#[serde(rename = "repeat_region")]
    RepeatRegion,
#[serde(rename = "TR_box")]
    TRBox,
    SNP,
}

impl Display for FeatureType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match *self {
            FeatureType::FivePrimeUtr => "5'UTR",
            FeatureType::FivePrimeUtrIntron => "5'UTR_intron",
            FeatureType::Exon => "exon",
            FeatureType::CdsIntron => "cds_intron",
            FeatureType::ThreePrimeUtr => "3'UTR",
            FeatureType::ThreePrimeUtrIntron => "3'UTR_intron",
            FeatureType::DGRepeat => "dg_repeat",
            FeatureType::DHRepeat => "dh_repeat",
            FeatureType::Gap => "gap",
            FeatureType::GeneGroup => "gene_group",
            FeatureType::LongTerminalRepeat => "long_terminal_repeat",
            FeatureType::LowComplexityRegion => "low_complexity_region",
            FeatureType::LTRRetrotransposon => "LTR_retrotransposon",
            FeatureType::MatingTypeRegion => "mating_type_region",
            FeatureType::NuclearMtPseudogene => "nuclear_mt_pseudogene",
            FeatureType::OriginOfReplication => "origin_of_replication",
            FeatureType::PolyASignalSequence => "polyA_signal_sequence",
            FeatureType::PolyASite => "polyA_site",
            FeatureType::Promoter => "promoter",
            FeatureType::Region => "region",
            FeatureType::RegionalCentromere => "regional_centromere",
            FeatureType::RegionalCentromereCentralCore => "regional_centromere_central_core",
            FeatureType::RegionalCentromereInnerRepeatRegion => "regional_centromere_inner_repeat_region",
            FeatureType::RepeatRegion => "repeat_region",
            FeatureType::TRBox => "TR_box",
            FeatureType::SNP => "SNP",
        })
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FeatureShort {
    pub feature_type: FeatureType,
    pub uniquename: RcString,
    pub location: ChromosomeLocation,
    pub residues: Residues,
}


#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TranscriptDetails {
    pub uniquename: TranscriptUniquename,
    pub location: ChromosomeLocation,
    pub parts: Vec<FeatureShort>,
    pub transcript_type: RcString,
    pub protein: Option<ProteinDetails>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub cds_location: Option<ChromosomeLocation>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GenotypeShort {
    pub display_uniquename: GenotypeUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<RcString>,
    pub expressed_alleles: Vec<ExpressedAllele>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GenotypeDetails {
    pub display_uniquename: GenotypeUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<RcString>,
    pub expressed_alleles: Vec<ExpressedAllele>,
    pub cv_annotations: OntAnnotationMap,
    pub references_by_uniquename: ReferenceShortOptionMap,
    pub genes_by_uniquename: GeneShortOptionMap,
    pub alleles_by_uniquename: HashMap<AlleleUniquename, AlleleShort>,
    pub terms_by_termid: TermShortOptionMap,
    pub annotation_details: IdOntAnnotationDetailMap,
}

impl Container for GenotypeDetails {
    fn container_type(&self) -> ContainerType {
        ContainerType::Genotype
    }
}

impl AnnotationContainer for GenotypeDetails {
    fn cv_annotations(&self) -> &OntAnnotationMap {
        &self.cv_annotations
    }
    fn cv_annotations_mut(&mut self) -> &mut OntAnnotationMap {
        &mut self.cv_annotations
    }
    fn annotation_details(&self) -> &IdOntAnnotationDetailMap {
        &self.annotation_details
    }
    fn terms_by_termid(&self) -> &TermShortOptionMap {
        &self.terms_by_termid
    }
    fn genes_by_uniquename(&self) -> &GeneShortOptionMap {
        &self.genes_by_uniquename
    }
    fn genotypes_by_uniquename(&self) -> Option<&HashMap<GenotypeUniquename, GenotypeShort>> {
        None
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExpressedAllele {
    #[serde(skip_serializing_if="Option::is_none")]
    pub expression: Option<RcString>,
    pub allele_uniquename: AlleleUniquename,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AlleleShort {
    pub uniquename: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<RcString>,
    pub allele_type: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub description: Option<RcString>,
    pub gene_uniquename: GeneUniquename,
}

pub type RelName = RcString;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneExProps {
    #[serde(skip_serializing_if="Option::is_none")]
    pub copies_per_cell: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub avg_copies_per_cell: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub scale: Option<RcString>,
}

pub type OntName = RcString;
pub type OntAnnotationMap = HashMap<OntName, Vec<OntTermAnnotations>>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TermAndRelation {
    pub termid: TermId,
    pub term_name: TermName,
    pub relation_name: RelName,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TermDetails {
    pub name: TermName,
    pub cv_name: CvName,
    pub annotation_feature_type: RcString,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub interesting_parents: HashSet<RcString>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub in_subsets: HashSet<RcString>,
    pub termid: TermId,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub synonyms: Vec<SynonymDetails>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub definition: Option<TermDef>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub direct_ancestors: Vec<TermAndRelation>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub genes_annotated_with: HashSet<GeneUniquename>,
    pub is_obsolete: bool,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub single_allele_genotype_uniquenames: HashSet<RcString>,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub cv_annotations: OntAnnotationMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub genes_by_uniquename: GeneShortOptionMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub genotypes_by_uniquename: HashMap<GenotypeUniquename, GenotypeShort>,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub alleles_by_uniquename: HashMap<AlleleUniquename, AlleleShort>,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub references_by_uniquename: ReferenceShortOptionMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub terms_by_termid: TermShortOptionMap,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub annotation_details: IdOntAnnotationDetailMap,
    pub gene_count: usize,
    pub genotype_count: usize,
    #[serde(skip_serializing_if="HashMap::is_empty", default)]
    pub xrefs: HashMap<RcString, TermXref>,
}

impl Container for TermDetails {
    fn container_type(&self) -> ContainerType {
        ContainerType::Term
    }
}

impl AnnotationContainer for TermDetails {
    fn cv_annotations(&self) -> &OntAnnotationMap {
        &self.cv_annotations
    }
    fn cv_annotations_mut(&mut self) -> &mut OntAnnotationMap {
        &mut self.cv_annotations
    }
    fn annotation_details(&self) -> &IdOntAnnotationDetailMap {
        &self.annotation_details
    }
    fn terms_by_termid(&self) -> &TermShortOptionMap {
        &self.terms_by_termid
    }
    fn genes_by_uniquename(&self) -> &GeneShortOptionMap {
        &self.genes_by_uniquename
    }
    fn genotypes_by_uniquename(&self) -> Option<&HashMap<GenotypeUniquename, GenotypeShort>> {
        Some(&self.genotypes_by_uniquename)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InteractionAnnotation {
    pub gene_uniquename: GeneUniquename,
    pub interactor_uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub evidence: Option<Evidence>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub reference_uniquename: Option<ReferenceUniquename>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub throughput: Option<Throughput>,
}
impl PartialEq for InteractionAnnotation {
    fn eq(&self, other: &Self) -> bool {
        if let Some(ref evidence) = self.evidence {
            if let Some(ref other_evidence) = other.evidence {
                return evidence == other_evidence;
            }
        }
        (&self.gene_uniquename, &self.interactor_uniquename) ==
            (&other.gene_uniquename, &other.interactor_uniquename)
    }
}
impl Eq for InteractionAnnotation { }
impl Ord for InteractionAnnotation {
    fn cmp(&self, other: &Self) -> Ordering {
        if let Some(ref evidence) = self.evidence {
            if let Some(ref other_evidence) = other.evidence {
                let order = evidence.cmp(other_evidence);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
        (&self.gene_uniquename, &self.interactor_uniquename)
            .cmp(&(&other.gene_uniquename, &other.interactor_uniquename))
    }
}
impl PartialOrd for InteractionAnnotation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OrthologAnnotation {
    pub gene_uniquename: GeneUniquename,
    pub ortholog_taxonid: u32,
    pub ortholog_uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub evidence: Option<Evidence>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub reference_uniquename: Option<ReferenceUniquename>,
}
impl PartialEq for OrthologAnnotation {
    fn eq(&self, other: &Self) -> bool {
        (&self.gene_uniquename, &self.ortholog_uniquename) ==
            (&other.gene_uniquename, &other.ortholog_uniquename)
    }
}
impl Eq for OrthologAnnotation { }
impl Ord for OrthologAnnotation {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.gene_uniquename, &self.ortholog_uniquename)
            .cmp(&(&other.gene_uniquename, &other.ortholog_uniquename))
    }
}
impl PartialOrd for OrthologAnnotation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ParalogAnnotation {
    pub gene_uniquename: GeneUniquename,
    pub paralog_uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub evidence: Option<Evidence>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub reference_uniquename: Option<ReferenceUniquename>,
}
impl PartialEq for ParalogAnnotation {
    fn eq(&self, other: &Self) -> bool {
        (&self.gene_uniquename, &self.paralog_uniquename) ==
            (&other.gene_uniquename, &other.paralog_uniquename)
    }
}
impl Eq for ParalogAnnotation { }
impl Ord for ParalogAnnotation {
    fn cmp(&self, other: &Self) -> Ordering {
        (&self.gene_uniquename, &self.paralog_uniquename)
            .cmp(&(&other.gene_uniquename, &other.paralog_uniquename))
    }
}
impl PartialOrd for ParalogAnnotation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Metadata {
    pub db_creation_datetime: RcString,
    pub export_prog_name: RcString,
    pub export_prog_version: RcString,
    pub gene_count: usize,
    pub term_count: usize,
    pub cv_versions: HashMap<RcString, RcString>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct APIAlleleDetails {
    pub gene: GeneUniquename,
    pub allele_type: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub expression: Option<RcString>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct APIGenotypeAnnotation {
    pub is_multi: bool,
    pub alleles: Vec<APIAlleleDetails>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct APIGeneSummary {
    pub uniquename: GeneUniquename,
    #[serde(skip_serializing_if="Option::is_none")]
    pub name: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub product: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub uniprot_identifier: Option<RcString>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub exact_synonyms: Vec<RcString>,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub dbxrefs: HashSet<RcString>,
    pub location: Option<ChromosomeLocation>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub transcripts: Vec<TranscriptDetails>,
    pub tm_domain_count: usize,
    pub exon_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GeneQueryTermData {
    Term(TermAndName),
    Other,
}

pub type GeneQueryAttrName = RcString;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneQueryData {
    pub gene_uniquename: GeneUniquename,
    pub deletion_viability: DeletionViability,
#[serde(skip_serializing_if="Option::is_none")]
    pub go_component: Option<GeneQueryTermData>,
#[serde(skip_serializing_if="Option::is_none")]
    pub go_process_superslim: Option<GeneQueryTermData>,
#[serde(skip_serializing_if="Option::is_none")]
    pub go_function: Option<GeneQueryTermData>,
#[serde(skip_serializing_if="Option::is_none")]
    pub characterisation_status: Option<RcString>,
#[serde(skip_serializing_if="Option::is_none")]
    pub taxonomic_distribution: Option<RcString>,
#[serde(skip_serializing_if="Option::is_none")]
    pub tmm: Option<PresentAbsent>,
    pub ortholog_taxonids: HashSet<u32>,
    // bin is None for RNA genes:
    pub protein_length_bin: Option<GeneQueryAttrName>,
#[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub subset_termids: HashSet<TermId>,
}

#[derive(Serialize, Deserialize, Copy, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InteractionType {
    Physical,
    Genetic
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct APIInteractor {
    pub interaction_type: InteractionType,
    pub interactor_uniquename: GeneUniquename,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct APIMaps {
    pub termid_genes: HashMap<TermId, HashSet<GeneUniquename>>,
    pub termid_genotype_annotation: HashMap<TermId, Vec<APIGenotypeAnnotation>>,
    pub gene_summaries: HashMap<GeneUniquename, APIGeneSummary>,
    pub gene_query_data_map: HashMap<GeneUniquename, GeneQueryData>,
    pub term_summaries: HashSet<TermShort>,
    pub genes: UniquenameGeneMap,
    pub gene_name_gene_map: HashMap<RcString, GeneUniquename>,
    pub genotypes: IdGenotypeMap,
    pub terms: HashMap<TermId, TermDetails>,
    pub interactors_of_genes: HashMap<GeneUniquename, Vec<APIInteractor>>,
    pub references: UniquenameReferenceMap,
    pub other_features: UniquenameFeatureShortMap,
    pub annotation_details: IdOntAnnotationDetailMap,
    pub chromosomes: ChrNameDetailsMap,
    pub term_subsets: IdTermSubsetMap,
    pub gene_subsets: IdGeneSubsetMap,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SolrTermSummary {
    pub id: TermId,
    pub name: TermName,
    pub cv_name: CvName,
    #[serde(skip_serializing_if="Option::is_none")]
    pub definition: Option<TermDef>,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub close_synonyms: Vec<RcString>,   // exact and narrow
    // a uniquified list of the words in all close synonyms
    pub close_synonym_words: RcString,
    #[serde(skip_serializing_if="Vec::is_empty", default)]
    pub distant_synonyms: Vec<RcString>, // broad and related
    // a uniquified list of the words in all distant synonyms
    pub distant_synonym_words: RcString,
    #[serde(skip_serializing_if="HashSet::is_empty", default)]
    pub interesting_parents: HashSet<RcString>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SolrReferenceSummary {
    pub id: RcString,
    #[serde(skip_serializing_if="Option::is_none")]
    pub title: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub citation: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub pubmed_abstract: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub authors: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub authors_abbrev: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub pubmed_publication_date: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub publication_year: Option<u32>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub approved_date: Option<RcString>,
    pub gene_count: usize,
    pub genotype_count: usize,
    pub annotation_count: usize,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_annotation_status: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_curator_name: Option<RcString>,
    #[serde(skip_serializing_if="Option::is_none")]
    pub canto_curator_role: Option<RcString>,
}

impl SolrReferenceSummary {
    pub fn from_reference_details(reference_details: &ReferenceDetails) -> SolrReferenceSummary {
        let pub_year_as_int: Option<u32> =
            if let Some(ref pub_year) = reference_details.publication_year {
                pub_year.parse().ok()
            } else {
                None
            };

        let annotation_count = reference_details.annotation_details.len() +
            reference_details.genetic_interactions.len() +
            reference_details.physical_interactions.len() +
            reference_details.ortholog_annotations.len() +
            reference_details.paralog_annotations.len();

        SolrReferenceSummary {
            id: reference_details.uniquename.clone(),
            title: reference_details.title.clone(),
            pubmed_abstract: reference_details.pubmed_abstract.clone(),
            citation: reference_details.citation.clone(),
            publication_year: pub_year_as_int,
            pubmed_publication_date: reference_details.pubmed_publication_date.clone(),
            authors: reference_details.authors.clone(),
            authors_abbrev: reference_details.authors_abbrev.clone(),
            approved_date: reference_details.approved_date.clone(),
            gene_count: reference_details.genes_by_uniquename.keys().len(),
            genotype_count: reference_details.genotypes_by_uniquename.keys().len(),
            annotation_count,
            canto_annotation_status: reference_details.canto_annotation_status.clone(),
            canto_curator_name: reference_details.canto_curator_name.clone(),
            canto_curator_role: reference_details.canto_curator_role.clone(),
        }
    }
}



#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SolrData {
    pub term_summaries: Vec<SolrTermSummary>,
    pub gene_summaries: Vec<GeneSummary>,
    pub reference_summaries: Vec<SolrReferenceSummary>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RecentReferences {
    // most recent papers from PubMed
    pub pubmed: Vec<ReferenceShort>,
    // most recent admin curated papers
    pub admin_curated: Vec<ReferenceShort>,
    // most recent community curated
    pub community_curated: Vec<ReferenceShort>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TermSubsetElement {
    pub name: RcString,
    pub termid: TermId,
    pub gene_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TermSubsetDetails {
    pub name: RcString,
    pub total_gene_count: usize, // total unique genes in all subsets
    pub elements: HashSet<TermSubsetElement>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GeneSubsetDetails {
    pub name: RcString,
    pub display_name: RcString,
    pub elements: HashSet<GeneUniquename>,
}

pub type IdTermSubsetMap = HashMap<RcString, TermSubsetDetails>;
pub type IdGeneSubsetMap = HashMap<RcString, GeneSubsetDetails>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Stats {
    pub gene_counts_by_taxonid: HashMap<OrganismTaxonId, usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WebData {
    pub metadata: Metadata,
    pub chromosomes: ChrNameDetailsMap,
    pub chromosome_summaries: Vec<ChromosomeShort>,
    pub recent_references: RecentReferences,
    pub all_community_curated: Vec<ReferenceShort>,
    pub all_admin_curated: Vec<ReferenceShort>,
    pub api_maps: APIMaps,
    pub solr_data: SolrData,
    pub search_gene_summaries: Vec<GeneSummary>,
    pub ont_annotations: Vec<OntAnnotation>,
    pub stats: Stats,
}

impl WebData {
    fn get_chromosomes(&self) -> &ChrNameDetailsMap {
        &self.chromosomes
    }

    fn create_dir(&self, output_dir: &str, dir_name: &str) -> String {
        let path = String::new() + output_dir + "/" + dir_name;
        create_dir_all(&path).unwrap_or_else(|why| {
            println!("Creating output directory failed: {:?}", why.kind());
        });
        path
    }

    fn write_chromosome_seq_chunks(&self, output_dir: &str, chunk_sizes: &[usize]) {
        for chunk_size in chunk_sizes {
            for (chromosome_uniquename, chromosome_details) in &self.chromosomes {
                let new_path_part = &format!("{}/sequence/{}", chromosome_uniquename, chunk_size);
                let chr_path = self.create_dir(output_dir, new_path_part);
                let mut index = 0;
                let max_index = chromosome_details.residues.len() / chunk_size;
                while index <= max_index {
                    let start_pos = index*chunk_size;
                    let end_pos = min(start_pos+chunk_size, chromosome_details.residues.len());
                    let chunk: String = chromosome_details.residues[start_pos..end_pos].into();
                    let file_name = format!("{}/chunk_{}", chr_path, index);
                    let f = File::create(file_name).expect("Unable to open file");
                    let mut writer = BufWriter::new(&f);
                    writer.write_all(chunk.as_bytes()).expect("Unable to write chromosome chunk");
                    index += 1;
                }
            }
        }
    }

    fn write_chromosome_json(&self, config: &Config, output_dir: &str) {
        let new_path = self.create_dir(output_dir, "chromosome");
        for (chromosome_uniquename, chromosome_details) in &self.chromosomes {
            let s = serde_json::to_string(&chromosome_details).unwrap();
            let file_name = format!("{}/{}.json", new_path, &chromosome_uniquename);
            let f = File::create(file_name).expect("Unable to open file");
            let mut writer = BufWriter::new(&f);
            writer.write_all(s.as_bytes()).expect("Unable to write chromosome JSON");
        }
        self.write_chromosome_seq_chunks(&new_path, &config.api_seq_chunk_sizes);
    }

    fn write_gene_summaries(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.search_gene_summaries).unwrap();
        let file_name = String::new() + output_dir + "/gene_summaries.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write gene_summaries.json");
    }

    fn write_metadata(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.metadata).unwrap();
        let file_name = String::new() + output_dir + "/metadata.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write metadata.json");
    }

    fn write_recent_references(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.recent_references).unwrap();
        let file_name = String::new() + output_dir + "/recent_references.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write recent references JSON");
    }

    fn write_all_community_curated(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.all_community_curated).unwrap();
        let file_name = String::new() + output_dir + "/community_curated_references.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write recent references JSON");
    }

    fn write_all_admin_curated(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.all_admin_curated).unwrap();
        let file_name = String::new() + output_dir + "/admin_curated_references.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write admin curated refs JSON");
    }

    fn write_api_maps(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.api_maps).unwrap();
        let file_name = String::new() + output_dir + "/api_maps.json.gz";
        let f = File::create(file_name).expect("Unable to open file");

        let mut compressor = GzEncoder::new(f, Compression::default());
        compressor.write_all(s.as_bytes()).unwrap();
        compressor.finish().unwrap();
    }

    fn write_solr_data(&self, output_dir: &str) {
        let new_path = self.create_dir(output_dir, "solr_data/");

        let terms = self.solr_data.term_summaries.clone();

        let terms_json_text = serde_json::to_string(&terms).unwrap();
        let terms_file_name = format!("{}/terms.json.gz", new_path);
        let terms_file = File::create(terms_file_name).expect("Unable to open file");

        let mut terms_compressor = GzEncoder::new(terms_file, Compression::default());
        terms_compressor.write_all(terms_json_text.as_bytes()).expect("Unable to write terms as JSON");
        terms_compressor.finish().expect("Unable to write terms as JSON");

        let genes = self.solr_data.gene_summaries.clone();

        let genes_json_text = serde_json::to_string(&genes).unwrap();
        let genes_file_name = format!("{}/genes.json.gz", new_path);
        let genes_file = File::create(genes_file_name).expect("Unable to open file");

        let mut genes_compressor = GzEncoder::new(genes_file, Compression::default());
        genes_compressor.write_all(genes_json_text.as_bytes()).expect("Unable to write genes as JSON");
        genes_compressor.finish().expect("Unable to write genes as JSON");

        let references = self.solr_data.reference_summaries.clone();

        let references_json_text = serde_json::to_string(&references).unwrap();
        let references_file_name = format!("{}/references.json.gz", new_path);
        let references_file = File::create(references_file_name).expect("Unable to open file");

        let mut references_compressor = GzEncoder::new(references_file, Compression::default());
        references_compressor.write_all(references_json_text.as_bytes()).expect("Unable to write references as JSON");
        references_compressor.finish().expect("Unable to write references as JSON");
    }

    fn write_subsets(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.api_maps.term_subsets).unwrap();
        let file_name = String::new() + output_dir + "/term_subsets.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write");

        let s = serde_json::to_string(&self.api_maps.gene_subsets).unwrap();
        let file_name = String::new() + output_dir + "/gene_subsets.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write");
    }

    fn write_feature_sequences(&self, output_dir: &str) {
        let make_seq_writer = |name: &str| {
            let file_name = String::new() + output_dir + "/" + name;
            let file = File::create(file_name).expect("Unable to open file");
            BufWriter::new(file)
        };

        let mut cds_writer = make_seq_writer("cds.fa");
        let mut cds_introns_writer = make_seq_writer("cds+introns.fa");
        let mut cds_introns_utrs_writer = make_seq_writer("cds+introns+utrs.fa");
        let mut introns_writer = make_seq_writer("introns_within_cds.fa");
        let mut five_prime_utrs_writer = make_seq_writer("five_prime_utrs.fa");
        let mut three_prime_utrs_writer = make_seq_writer("three_prime_utrs.fa");
        let mut peptide_writer = make_seq_writer("peptide.fa");

        for (gene_uniquename, gene_details) in &self.api_maps.genes {
            if let Some(transcript) = gene_details.transcripts.get(0) {
                let mut cds_seq = String::new();
                let mut cds_introns_seq = String::new();
                let mut cds_introns_utrs_seq = String::new();
                let mut five_prime_utr_seq = String::new();
                let mut three_prime_utr_seq = String::new();
                for part in &transcript.parts {
                    if part.feature_type == FeatureType::Exon {
                        cds_seq += &part.residues;
                        cds_introns_seq += &part.residues;
                    }
                    if part.feature_type == FeatureType::CdsIntron {
                        cds_introns_seq += &part.residues;
                    }
                    if part.feature_type == FeatureType::CdsIntron {
                        write_as_fasta(&mut introns_writer, &part.uniquename,
                                       Some(String::from(gene_uniquename)),
                                       &part.residues);
                    }
                    cds_introns_utrs_seq += &part.residues;
                    if part.feature_type == FeatureType::FivePrimeUtr {
                        five_prime_utr_seq += &part.residues;
                    }
                    if part.feature_type == FeatureType::ThreePrimeUtr {
                        three_prime_utr_seq += &part.residues;
                    }
                }

                write_as_fasta(&mut cds_writer, gene_uniquename, None, &cds_seq);
                write_as_fasta(&mut cds_introns_writer, gene_uniquename, None, &cds_introns_seq);
                write_as_fasta(&mut cds_introns_utrs_writer,
                               gene_uniquename, None, &cds_introns_utrs_seq);
                if !five_prime_utr_seq.is_empty() {
                    write_as_fasta(&mut five_prime_utrs_writer,
                                   gene_uniquename, None, &five_prime_utr_seq);
                }
                if !three_prime_utr_seq.is_empty() {
                    write_as_fasta(&mut three_prime_utrs_writer,
                                   gene_uniquename, None, &three_prime_utr_seq);
                }
                if let Some(ref protein) = transcript.protein {
                    let name_and_product =
                        if gene_details.name.is_some() || gene_details.product.is_some() {
                            let mut buf = String::new();
                            if let Some(ref name) = gene_details.name {
                                buf.push_str(name);
                            }
                            buf.push_str("|");
                            if let Some(ref product) = gene_details.product {
                                buf.push_str(product);
                            }
                            Some(buf.to_owned())
                        } else {
                            None
                        };
                    write_as_fasta(&mut peptide_writer, &(gene_uniquename.to_owned() + ":pep"),
                                   name_and_product, &protein.sequence);
                }
            }
        }

        cds_writer.flush().unwrap();
        cds_introns_writer.flush().unwrap();
        cds_introns_utrs_writer.flush().unwrap();
        introns_writer.flush().unwrap();
        peptide_writer.flush().unwrap();
        five_prime_utrs_writer.flush().unwrap();
        three_prime_utrs_writer.flush().unwrap();
    }

    pub fn write_chromosome_sequences(&self, config: &Config, output_dir: &str) {
        let make_seq_writer = |name: &str| {
            let file_name = String::new() + output_dir + "/" + name;
            let file = File::create(file_name).expect("Unable to open file");
            BufWriter::new(file)
        };

        if let Some(load_org) = config.load_organism() {
            let load_org_name = load_org.full_name();
            let chromosomes_file_name = load_org_name.clone() + "_all_chromosomes.fa";
            let mut chromosomes_writer = make_seq_writer(&chromosomes_file_name);

            for (uniquename, details) in &self.chromosomes {
                let chr_config = config.find_chromosome_config(uniquename);
                write_as_fasta(&mut chromosomes_writer, &chr_config.export_id,
                               Some(load_org_name.clone()), &details.residues);
                let this_chr_file_name =
                    load_org_name.clone() + "_" + &chr_config.export_file_id + ".fa";
                let mut this_chr_writer = make_seq_writer(&this_chr_file_name);
                write_as_fasta(&mut this_chr_writer, &chr_config.export_id,
                               Some(load_org_name.clone()), &details.residues);
                this_chr_writer.flush().unwrap();

            }

            chromosomes_writer.flush().unwrap();
        }
    }

    fn write_chromosome_summaries(&self, output_dir: &str) {
        let s = serde_json::to_string(&self.chromosome_summaries).unwrap();
        let file_name = String::new() + output_dir + "/chromosome_summaries.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write chromosome_summaries.json");
    }

    fn write_gene_id_table(&self, config: &Config, output_dir: &str) -> Result<(), io::Error> {
        let gene_file_name = output_dir.to_owned() + "/sysID2product.tsv";
        let rna_file_name = output_dir.to_owned() + "/sysID2product.rna.tsv";
        let pseudogenes_file_name = output_dir.to_owned() + "/pseudogeneIDs.tsv";
        let all_names_file_name = output_dir.to_owned() + "/gene_IDs_names.tsv";
        let all_ids_file_name = output_dir.to_owned() + "/gene_IDs_names_products.tsv";

        let gene_file = File::create(gene_file_name).expect("Unable to open file");
        let rna_file = File::create(rna_file_name).expect("Unable to open file");
        let pseudogenes_file = File::create(pseudogenes_file_name).expect("Unable to open file");
        let all_names_file = File::create(all_names_file_name).expect("Unable to open file");
        let all_ids_file = File::create(all_ids_file_name).expect("Unable to open file");

        let mut gene_writer = BufWriter::new(&gene_file);
        let mut rna_writer = BufWriter::new(&rna_file);
        let mut pseudogenes_writer = BufWriter::new(&pseudogenes_file);
        let mut all_names_writer = BufWriter::new(&all_names_file);
        let mut all_ids_writer = BufWriter::new(&all_ids_file);

        let db_version = format!("# Chado database date: {}\n", self.metadata.db_creation_datetime);
        gene_writer.write_all(db_version.as_bytes())?;
        rna_writer.write_all(db_version.as_bytes())?;
        pseudogenes_writer.write_all(db_version.as_bytes())?;
        all_names_writer.write_all(db_version.as_bytes())?;

        for gene_details in self.api_maps.genes.values() {
            if let Some(load_org_taxonid) = config.load_organism_taxonid {
                if gene_details.taxonid != load_org_taxonid {
                    continue;
                }
            }

            let synonyms =
                gene_details.synonyms.iter().filter(|synonym| {
                    synonym.synonym_type == "exact"
                })
                .map(|synonym| synonym.name.to_string())
                .collect::<Vec<String>>()
                .join(",");

            let line = format!("{}\t{}\t{}\n",
                               gene_details.uniquename,
                               gene_details.name.clone().unwrap_or_else(|| RcString::new()),
                               synonyms);

            let gene_name = if let Some(ref gene_details_name) = gene_details.name {
                gene_details_name.clone()
            } else {
                RcString::new()
            };

            let gene_product = if let Some(ref gene_details_product) = gene_details.product {
                gene_details_product.clone()
            } else {
                RcString::new()
            };

            let line_with_product = format!("{}\t{}\t{}\t{}\n",
                                            gene_details.uniquename,
                                            gene_name,
                                            synonyms,
                                            gene_product);

            all_names_writer.write_all(line.as_bytes())?;

            if gene_details.feature_type == "pseudogene" {
                pseudogenes_writer.write_all(line.as_bytes())?;
            } else {
                if gene_details.feature_type == "mRNA gene" {
                    gene_writer.write_all(line_with_product.as_bytes())?;
                } else {
                    if gene_details.feature_type.contains("RNA") {
                        rna_writer.write_all(line_with_product.as_bytes())?;
                    }
                }
            }

            let uniprot_id =
                if let Some(ref gene_uniprot_id) = gene_details.uniprot_identifier {
                    gene_uniprot_id
                } else {
                    ""
                };

            let chromosome_name =
                if let Some(ref loc) = gene_details.location {
                    &loc.chromosome_name
                } else {
                    ""
                };

            let gene_type =
                if gene_details.feature_type == "mRNA gene" {
                    "protein coding gene"
                } else {
                    &gene_details.feature_type
                };

            let all_ids_line = format!("{}\t{}:{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                                       gene_details.uniquename,
                                       config.database_name,
                                       gene_details.uniquename,
                                       gene_name,
                                       chromosome_name,
                                       gene_product,
                                       uniprot_id,
                                       gene_type,
                                       synonyms);
            all_ids_writer.write_all(all_ids_line.as_bytes())?;
        }

        gene_writer.flush()?;
        rna_writer.flush()?;
        pseudogenes_writer.flush()?;
        all_names_writer.flush()?;

        Ok(())
    }

    fn write_protein_features(&self, config: &Config, output_dir: &str)
                              -> Result<(), io::Error>
    {
        let peptide_stats_name = format!("{}/PeptideStats.tsv", output_dir);
        let peptide_stats_file = File::create(peptide_stats_name).expect("Unable to open file");
        let mut peptide_stats_writer = BufWriter::new(&peptide_stats_file);

        let peptide_stats_header = "Systematic_ID\tMass (kDa)\tpI\tCharge\tResidues\tCAI\n";
        peptide_stats_writer.write_all(peptide_stats_header.as_bytes())?;
 
        let protein_features_name = format!("{}/ProteinFeatures.tsv", output_dir);
        let protein_features_file = File::create(protein_features_name).expect("Unable to open file");
        let mut protein_features_writer = BufWriter::new(&protein_features_file);

        let aa_composition_name = format!("{}/aa_composition.tsv", output_dir);
        let aa_composition_file = File::create(aa_composition_name).expect("Unable to open file");
        let mut aa_composition_writer = BufWriter::new(&aa_composition_file);

        let protein_features_header =
            "systematic_id\tgene_name\tpeptide_id\tdomain_id\tdatabase\tseq_start\tseq_end\n";
        protein_features_writer.write_all(protein_features_header.as_bytes())?;

        let db_display_name = |db_alias: &str| {
            if let Some(name) = config.extra_database_aliases.get(&db_alias.to_lowercase()) {
                name.clone()
            } else {
                RcString::from(db_alias)
            }
        };

        type AAComposition = HashMap<char, u32>;

        let mut total_composition: AAComposition = HashMap::new();

        let prot_composition =
            |total_composition: &mut AAComposition, protein: &ProteinDetails|
        {
            let mut composition = HashMap::new();
            for c in protein.sequence.chars() {
                let count = composition.entry(c).or_insert(0);
                *count += 1;
                let total_count = total_composition.entry(c).or_insert(0);
                *total_count += 1;
            }
            composition
        };

        let mut compositions_to_write = vec![];

        for (gene_uniquename, gene_details) in &self.api_maps.genes {
            if let Some(transcript) = gene_details.transcripts.get(0) {
                if let Some(ref protein) = transcript.protein {
                    let line = format!("{}\t{:.2}\t{}\t{}\t{}\t{}\n",
                                       gene_uniquename, protein.molecular_weight,
                                       protein.isoelectric_point,
                                       protein.charge_at_ph7,
                                       protein.sequence.len() - 1,
                                       protein.codon_adaptation_index);
                    peptide_stats_writer.write_all(line.as_bytes())?;

                    let gene_name = gene_details.name.clone().unwrap_or_else(|| RcString::new());
                    for interpro_match in &gene_details.interpro_matches {
                        let line_start = format!("{}\t{}\t{}\t{}\t{}",
                                                 gene_uniquename, gene_name,
                                                 protein.uniquename, interpro_match.id,
                                                 db_display_name(&interpro_match.dbname));
                        for location in &interpro_match.locations {
                            let line = format!("{}\t{}\t{}\n", line_start,
                                               location.start, location.end);
                            protein_features_writer.write_all(line.as_bytes())?;
                        }
                    }

                    let composition = prot_composition(&mut total_composition, &protein);

                    compositions_to_write.push((gene_uniquename.clone(), composition));
                }
            }

        }

        let mut all_composition_aa: Vec<char> = vec![];

        for ch in total_composition.keys() {
            if *ch != '*' {
                all_composition_aa.push(*ch);
            }
        }

        all_composition_aa.sort();

        let all_composition_string =
            all_composition_aa.iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>().join("\t");

        let composition_header = "Systematic_ID\t".to_owned() +
            &all_composition_string + "\n";
        aa_composition_writer.write_all(composition_header.as_bytes())?;

        let composition_line = |first_col_string: RcString, comp: &AAComposition| {
            let mut line = String::from(first_col_string);

            for ch in &all_composition_aa {
                line.push_str("\t");
                if let Some(count) = comp.get(ch) {
                    line.push_str(&count.to_string());
                } else {
                    line.push_str("0");
                }
            }
            line.push_str("\n");
            line
        };

        for (gene_uniquename, comp) in compositions_to_write.drain(0..) {
            let line = composition_line(gene_uniquename, &comp);
            aa_composition_writer.write_all(line.as_bytes())?;
        }

        let composition_total_line =
            composition_line(RcString::from("total"), &total_composition);
        aa_composition_writer.write_all(composition_total_line.as_bytes())?;

        peptide_stats_writer.flush()?;

        Ok(())
    }

    fn write_feature_coords(&self, config: &Config, output_dir: &str)
                            -> Result<(), io::Error>
    {
        let write_line =
            |uniquename: &str, location: &ChromosomeLocation,
             writer: &mut BufWriter<&File>| {
                let display_strand =
                    if location.strand == Strand::Forward {1} else {-1};
                let line = format!("{}\t{}\t{}\t{}\n",
                                   uniquename, location.start_pos,
                                   location.end_pos, display_strand);
                writer.write(line.as_bytes())
        };

        for (chr_uniquename, chr_details) in &self.chromosomes {
            if let Some(load_org_taxonid) = config.load_organism_taxonid {
                if chr_details.taxonid != load_org_taxonid {
                    continue;
                }
            }

            let gene_file_name = format!("{}/{}.gene.coords.tsv", output_dir, chr_uniquename);
            let cds_file_name = format!("{}/{}.cds.coords.tsv", output_dir, chr_uniquename);
            let exon_file_name = format!("{}/{}.exon.coords.tsv", output_dir, chr_uniquename);

            let gene_file = File::create(gene_file_name).expect("Unable to open file");
            let cds_file = File::create(cds_file_name).expect("Unable to open file");
            let exon_file = File::create(exon_file_name).expect("Unable to open file");

            let mut gene_writer = BufWriter::new(&gene_file);
            let mut cds_writer = BufWriter::new(&cds_file);
            let mut exon_writer = BufWriter::new(&exon_file);

            for gene_uniquename in &chr_details.gene_uniquenames {
                let gene = &self.api_maps.genes[gene_uniquename];
                if let Some(ref gene_location) = gene.location {
                    write_line(gene_uniquename, gene_location, &mut gene_writer)?;

                    for transcript in &gene.transcripts {
                        if let Some(ref cds_location) = transcript.cds_location {
                            write_line(gene_uniquename, cds_location, &mut cds_writer)?;
                        }

                        let is_forward =
                            transcript.parts[0].location.strand == Strand::Forward;

                        let parts: Vec<FeatureShort> = if is_forward {
                            transcript.parts.iter().cloned().filter(|part| {
                                part.feature_type == FeatureType::Exon ||
                                    part.feature_type == FeatureType::FivePrimeUtr ||
                                    part.feature_type == FeatureType::ThreePrimeUtr
                            }).collect()
                        } else {
                            transcript.parts.iter().cloned().rev().filter(|part| {
                                part.feature_type == FeatureType::Exon ||
                                    part.feature_type == FeatureType::FivePrimeUtr ||
                                    part.feature_type == FeatureType::ThreePrimeUtr
                            }).collect()
                        };

                        // merge Exon and (Three|Five)PrimeUtrs that abut 
                        let mut merged_locs: Vec<ChromosomeLocation> = vec![];

                        for part in parts {
                            if let Some(prev) = merged_locs.pop() {
                                if prev.end_pos + 1 == part.location.start_pos {
                                    merged_locs.push(ChromosomeLocation {
                                        start_pos: prev.start_pos,
                                        end_pos: part.location.end_pos,
                                        chromosome_name: prev.chromosome_name,
                                        strand: prev.strand,
                                        phase: prev.phase,
                                    });
                                } else {
                                    merged_locs.push(prev);
                                    merged_locs.push(part.location);
                                }
                            } else {
                                merged_locs.push(part.location);
                            }
                        }

                        for loc in merged_locs {
                            write_line(gene_uniquename, &loc, &mut exon_writer)?;
                        }
                    }
                }
            }

            gene_writer.flush()?;
            cds_writer.flush()?;
            exon_writer.flush()?;
        }

        Ok(())
    }

    pub fn write_gff(&self, config: &Config, output_dir: &str)
                         -> Result<(), io::Error>
    {
        if let Some(load_org) = config.load_organism() {
            let load_org_name = load_org.full_name();

            let all_gff_name = format!("{}/{}_all_chromosomes.gff3", output_dir, load_org_name);
            let all_gff_file = File::create(all_gff_name).expect("Unable to open file");
            let mut all_gff_writer = BufWriter::new(&all_gff_file);

            let forward_features_gff_name =
                format!("{}/{}_all_chromosomes_forward_strand.gff3", output_dir, load_org_name);
            let forward_features_gff_file = File::create(forward_features_gff_name).expect("Unable to open file");
            let mut forward_features_gff_writer = BufWriter::new(&forward_features_gff_file);

            let reverse_features_gff_name =
                format!("{}/{}_all_chromosomes_reverse_strand.gff3", output_dir, load_org_name);
            let reverse_features_gff_file = File::create(reverse_features_gff_name).expect("Unable to open file");
            let mut reverse_features_gff_writer = BufWriter::new(&reverse_features_gff_file);

            let unstranded_features_gff_name =
                format!("{}/{}_all_chromosomes_unstranded.gff3", output_dir, load_org_name);
            let unstranded_features_gff_file = File::create(unstranded_features_gff_name).expect("Unable to open file");
            let mut unstranded_features_gff_writer = BufWriter::new(&unstranded_features_gff_file);

            all_gff_writer.write_all(b"##gff-version 3\n")?;
            forward_features_gff_writer.write_all(b"##gff-version 3\n")?;
            reverse_features_gff_writer.write_all(b"##gff-version 3\n")?;
            unstranded_features_gff_writer.write_all(b"##gff-version 3\n")?;

            let mut chr_writers = HashMap::new();

            let make_chr_gff_writer = |export_name: &str| {
                let file_name = String::new() +
                    output_dir + "/" + &load_org_name + "_" + export_name + ".gff3";
                let file = File::create(file_name).expect("Unable to open file");
                BufWriter::new(file)
            };

            for uniquename in self.chromosomes.keys() {
                let chr_config = config.find_chromosome_config(uniquename);
                chr_writers.insert(uniquename, make_chr_gff_writer(&chr_config.export_file_id));
            }

            for gene_details in self.api_maps.genes.values() {
                if let Some(ref gene_loc) = gene_details.location {
                    let chromosome_name = &gene_loc.chromosome_name;
                    let chromosome_export_id =
                        &config.find_chromosome_config(chromosome_name).export_id;
                    let gene_gff_lines =
                        format_gene_gff(chromosome_export_id, &config.database_name, &gene_details);
                    for gff_line in gene_gff_lines {
                        all_gff_writer.write_all(gff_line.as_bytes())?;
                        all_gff_writer.write_all(b"\n")?;

                        match gene_loc.strand {
                            Strand::Forward => {
                                forward_features_gff_writer.write_all(gff_line.as_bytes())?;
                                forward_features_gff_writer.write_all(b"\n")?;
                            },
                            Strand::Reverse => {
                                reverse_features_gff_writer.write_all(gff_line.as_bytes())?;
                                reverse_features_gff_writer.write_all(b"\n")?;
                            }
                            Strand::Unstranded => {
                                unstranded_features_gff_writer.write_all(gff_line.as_bytes())?;
                                unstranded_features_gff_writer.write_all(b"\n")?;
                            }
                        }

                        if let Some(ref mut writer) = chr_writers.get_mut(chromosome_name) {
                            writer.write_all(gff_line.as_bytes())?;
                            writer.write_all(b"\n")?;
                        }
                    }
                }
            }

            for feature_short in self.api_maps.other_features.values() {
                let chromosome_name = &feature_short.location.chromosome_name;
                let chromosome_export_id =
                    &config.find_chromosome_config(chromosome_name).export_id;
                let gff_lines =
                    format_misc_feature_gff(&chromosome_export_id, &config.database_name,
                                            &feature_short);
                for gff_line in gff_lines {
                    all_gff_writer.write_all(gff_line.as_bytes())?;
                    all_gff_writer.write_all(b"\n")?;

                    match feature_short.location.strand {
                        Strand::Forward => {
                            forward_features_gff_writer.write_all(gff_line.as_bytes())?;
                            forward_features_gff_writer.write_all(b"\n")?;
                        },
                        Strand::Reverse => {
                            reverse_features_gff_writer.write_all(gff_line.as_bytes())?;
                            reverse_features_gff_writer.write_all(b"\n")?;
                        }
                        Strand::Unstranded => {
                            unstranded_features_gff_writer.write_all(gff_line.as_bytes())?;
                            unstranded_features_gff_writer.write_all(b"\n")?;
                        }
                    }

                    if let Some(ref mut writer) = chr_writers.get_mut(chromosome_name) {
                        writer.write_all(gff_line.as_bytes())?;
                        writer.write_all(b"\n")?;
                    }
                }
            }

            for writer in chr_writers.values_mut() {
                writer.flush().unwrap();
            }
        }

        Ok(())
    }

    pub fn write_macromolecular_complexes(&self, config: &Config, output_dir: &str)
                                          -> Result<(), io::Error>
    {
        let mut complex_data: HashMap<(TermShort, GeneShort, RcString), _> = HashMap::new();

        let no_evidence = RcString::from("NO_EVIDENCE");

        let make_key = |annotation: &OntAnnotation| {
            let evidence = annotation.evidence.clone().unwrap_or_else(|| no_evidence.clone());
            (annotation.term_short.clone(), annotation.genes.iter().next().unwrap().clone(),
             evidence)
        };

        if let Some(ref complexes_config) = config.file_exports.macromolecular_complexes {
            let check_parent_term = |el: &RcString| {
                *el == complexes_config.parent_complex_termid
            };
            'TERM: for annotation in &self.ont_annotations {
                let term_short = &annotation.term_short;
                let termid = &term_short.termid;

                if complexes_config.excluded_terms.contains(termid.as_ref()) {
                    continue 'TERM;
                }
                if !term_short.interesting_parents.iter().any(check_parent_term) {
                    continue 'TERM;
                }

                let key: (TermShort, GeneShort, RcString) = make_key(annotation);
                complex_data.entry(key)
                    .or_insert_with(Vec::new)
                    .push((annotation.reference_short.clone(), annotation.assigned_by.clone()));
            }
        }

        let complexes_file_name = format!("{}/Complex_annotation.tsv", output_dir);
        let complexes_file = File::create(complexes_file_name).expect("Unable to open file");
        let mut complexes_writer = BufWriter::new(&complexes_file);

        let header = "acc\tGO_name\tsystematic_id\tsymbol\tgene_product_description\tevidence_code\tsource\tassigned_by";

        complexes_writer.write_all(header.as_bytes())?;
        complexes_writer.write_all(b"\n")?;

        let mut lines = vec![];

        for (key, values) in complex_data.drain() {
            let (term_short, gene_short, evidence) = key;
            let mut refs = HashSet::new();
            let mut assigned_bys = HashSet::new();
            for (maybe_ref_short, maybe_assigned_by) in values {
                if let Some(ref_short) = maybe_ref_short {
                    refs.insert(ref_short.uniquename);
                }
                if let Some(assigned_by) = maybe_assigned_by {
                    assigned_bys.insert(assigned_by);
                }
            }

            let mut refs_vec = refs.into_iter().collect::<Vec<_>>();
            refs_vec.sort();
            let mut assigned_bys_vec = assigned_bys.into_iter().collect::<Vec<_>>();
            assigned_bys_vec.sort();

            let refs_string = refs_vec.join(",");
            let assigned_by_string = assigned_bys_vec.join(",");

            let line_bits = vec![term_short.termid.as_str(), &term_short.name.as_str(),
                                 gene_short.uniquename.as_str(),
                                 gene_short.name.as_ref().map(RcString::as_str)
                                   .unwrap_or_else(|| gene_short.uniquename.as_str()),
                                 gene_short.product.as_ref().map(RcString::as_str).unwrap_or_else(|| ""),
                                 evidence.as_str(), refs_string.as_str(),
                                 assigned_by_string.as_str()];

            lines.push(line_bits.join("\t"));
        }

        lines.sort();

        for line in lines.drain(0..) {
            complexes_writer.write_all(line.as_bytes())?;
            complexes_writer.write_all(b"\n")?
        }

        Ok(())
    }

    fn write_rnacentral(&self, config: &Config, output_dir: &str) -> Result<(), io::Error> {
        if config.file_exports.rnacentral.is_some() {
            let rnacentral_file_name = format!("{}/rnacentral.json", output_dir);
            let rnacentral_file = File::create(rnacentral_file_name).expect("Unable to open file");
            let mut rnacentral_writer = BufWriter::new(&rnacentral_file);
            let rnacentral_struct = make_rnacentral_struct(&config, &self.api_maps.genes);
            let s = serde_json::to_string(&rnacentral_struct).unwrap();

            rnacentral_writer.write_all(s.as_bytes())?;
            rnacentral_writer.write_all(b"\n")?;

            Ok(())
        } else {
            Ok(())
        }
    }

    pub fn write_deletion_viability(&self, config: &Config, output_dir: &str)
                                    -> Result<(), io::Error>
    {
        let deletion_viability_file_name = output_dir.to_owned() + "/FYPOviability.tsv";
        let deletion_viability_file =
            File::create(deletion_viability_file_name).expect("Unable to open file");
        let mut deletion_viability_writer = BufWriter::new(&deletion_viability_file);

        for gene_details in self.api_maps.genes.values() {
            if let Some(load_org_taxonid) = config.load_organism_taxonid {
                if gene_details.taxonid != load_org_taxonid {
                    continue;
                }
            }

            let line = format!("{}\t{}\n",
                               gene_details.uniquename,
                               match gene_details.deletion_viability {
                                   DeletionViability::Viable => "viable",
                                   DeletionViability::Inviable => "inviable",
                                   DeletionViability::DependsOnConditions =>
                                       "condition-dependent",
                                   DeletionViability::Unknown => "unknown",
                               });

            deletion_viability_writer.write_all(line.as_bytes())?;
        }

        deletion_viability_writer.flush()?;

        Ok(())
    }

    pub fn write_slim_ids_and_names(&self, config: &Config, output_dir: &str)
                                       -> Result<(), io::Error> {
        for (slim_name, slim_config) in &config.slims {
            let slim_file_name = format!("{}/{}_ids_and_names.tsv", output_dir, slim_name);

            let slim_file = File::create(slim_file_name).expect("Unable to open file");
            let mut slim_writer = BufWriter::new(&slim_file);

            for term_and_name in &slim_config.terms {
                let line = format!("{}\t{}\n", term_and_name.termid, term_and_name.name);

                slim_writer.write(line.as_bytes())?;
            }
        }

        Ok(())
    }

    pub fn write_transmembrane_domains(&self, config: &Config, output_dir: &str)
                                       -> Result<(), io::Error> {
        let tm_domain_file_name =
            output_dir.to_owned() + "/transmembrane_domain_coords_and_seqs.tsv";
        let tm_domain_file =
            File::create(tm_domain_file_name).expect("Unable to open file");
        let mut tm_domain_writer = BufWriter::new(&tm_domain_file);

        let coords_and_seqs = |coords: &[(usize, usize)], prot_seq: &str| {
            let mut coords_strings = vec![];
            let mut seqs = vec![];
            for (start, end) in coords {
                coords_strings.push(format!("{}..{}", start, end));
                let seq = &prot_seq[start-1..*end];
                seqs.push(seq.clone());
            }
            (coords_strings.join(","), seqs.join(","))
        };

        let star_re = Regex::new(r"\*$").unwrap();

        let format_one_gene = |gene_details: &GeneDetails, prot_seq: &str| {
            let prot_seq = star_re.replace_all(prot_seq, "");
            let (coords, seqs) =
                coords_and_seqs(&gene_details.tm_domain_coords, &prot_seq);
            format!("{}\t{}\t{}\t{}\t{}\n",
                    gene_details.uniquename,
                    gene_details.name.as_ref().map(|n| n.as_str()).unwrap_or(""),
                    prot_seq,
                    coords, seqs)
        };

        for gene_details in self.api_maps.genes.values() {
            if let Some(load_org_taxonid) = config.load_organism_taxonid {
                if gene_details.taxonid != load_org_taxonid {
                    continue;
                }
            }

            if gene_details.tm_domain_coords.len() == 0 {
                continue;
            }

            if let Some(transcript) = gene_details.transcripts.get(0) {
                if let Some(ref protein) = transcript.protein {
                    let line = format_one_gene(gene_details, &protein.sequence);

                    tm_domain_writer.write_all(line.as_bytes())?;
                }
            }
        }

        tm_domain_writer.flush()?;

        Ok(())
    }

    pub fn write_stats(&self, output_dir: &str) -> Result<(), io::Error> {
        let s = serde_json::to_string(&self.stats).unwrap();
        let file_name = String::new() + output_dir + "/stats.json";
        let f = File::create(file_name).expect("Unable to open file");
        let mut writer = BufWriter::new(&f);
        writer.write_all(s.as_bytes()).expect("Unable to write stats.json");

        Ok(())
    }

    pub fn write(&self, config: &Config, output_dir: &str) -> Result<(), io::Error> {
        let web_json_path = self.create_dir(output_dir, "web-json");

        self.write_chromosome_json(config, &web_json_path);
        println!("wrote {} chromosomes", self.get_chromosomes().len());
        self.write_gene_summaries(&web_json_path);
        self.write_chromosome_summaries(&web_json_path);
        println!("wrote summaries");
        self.write_metadata(&web_json_path);
        println!("wrote metadata");
        self.write_recent_references(&web_json_path);
        self.write_all_community_curated(&web_json_path);
        self.write_all_admin_curated(&web_json_path);
        println!("wrote references");
        self.write_api_maps(&web_json_path);
        self.write_solr_data(&web_json_path);
        println!("wrote search data");
        self.write_subsets(&web_json_path);
        println!("wrote subsets");

        let fasta_path = self.create_dir(output_dir, "fasta");
        let feature_sequences_path = self.create_dir(&fasta_path, "feature_sequences");
        self.write_feature_sequences(&feature_sequences_path);
        let chromosomes_path = self.create_dir(&fasta_path, "chromosomes");
        self.write_chromosome_sequences(config, &chromosomes_path);
        println!("wrote fasta");

        let misc_path = self.create_dir(output_dir, "misc");
        self.write_gene_id_table(&config, &misc_path)?;
        self.write_protein_features(&config, &misc_path)?;
        self.write_feature_coords(&config, &misc_path)?;
        self.write_macromolecular_complexes(&config, &misc_path)?;
        self.write_rnacentral(&config, &misc_path)?;
        self.write_deletion_viability(&config, &misc_path)?;
        self.write_slim_ids_and_names(&config, &misc_path)?;
        self.write_transmembrane_domains(&config, &misc_path)?;

        self.write_stats(&web_json_path)?;

        let gff_path = self.create_dir(output_dir, "gff");
        self.write_gff(&config, &gff_path)?;

        Ok(())
    }

    pub fn store_jsonb(&self, conn: &Connection) {
        let trans = conn.transaction().unwrap();

        for (uniquename, gene_details) in &self.api_maps.genes {
            let serde_value = serde_json::value::to_value(&gene_details).unwrap();
            trans.execute("INSERT INTO web_json.gene (uniquename, data) values ($1, $2)",
                          &[&uniquename.as_str(), &serde_value]).unwrap();
        }
        for (uniquename, ref_details) in &self.api_maps.references {
            let serde_value = serde_json::value::to_value(&ref_details).unwrap();
            trans.execute("INSERT INTO web_json.reference (uniquename, data) values ($1, $2)",
                          &[&uniquename.as_str(), &serde_value]).unwrap();
        }
        for (termid, term_details) in &self.api_maps.terms {
            let serde_value = serde_json::value::to_value(&term_details).unwrap();
            trans.execute("INSERT INTO web_json.term (termid, data) values ($1, $2)",
                          &[&termid.as_str(), &serde_value]).unwrap();
        }

        trans.execute("CREATE INDEX gene_jsonb_idx ON web_json.gene USING gin (data jsonb_path_ops)", &[]).unwrap();
        trans.execute("CREATE INDEX gene_jsonb_name_idx ON web_json.gene USING gin ((data->>'name') gin_trgm_ops);", &[]).unwrap();
        trans.execute("CREATE INDEX term_jsonb_idx ON web_json.term USING gin (data jsonb_path_ops)", &[]).unwrap();
        trans.execute("CREATE INDEX term_jsonb_name_idx ON web_json.term USING gin ((data->>'name') gin_trgm_ops);", &[]).unwrap();
        trans.execute("CREATE INDEX reference_jsonb_idx ON web_json.reference USING gin (data jsonb_path_ops)", &[]).unwrap();
        trans.execute("CREATE INDEX reference_jsonb_title_idx ON web_json.reference USING gin ((data->>'title') gin_trgm_ops);", &[]).unwrap();

        trans.commit().unwrap();
    }
}

