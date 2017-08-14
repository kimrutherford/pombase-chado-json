use std::collections::hash_set::HashSet;
use std::iter::FromIterator;

use api::server_data::ServerData;
use api::result::*;
use web::data::APIGeneSummary;

use types::GeneUniquename;

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub enum IntRangeType {
#[serde(rename = "genome_range_contains")]
    GenomeRangeContains,
#[serde(rename = "protein_length")]
    ProteinLength,
#[serde(rename = "tm_domain_count")]
    TMDomainCount,
#[serde(rename = "exon_count")]
    ExonCount,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub enum FloatRangeType {
#[serde(rename = "protein_mol_weight")]
    ProteinMolWeight,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub enum SingleOrMultiAllele {
#[serde(rename = "single")]
    Single,
#[serde(rename = "multi")]
    Multi,
#[serde(rename = "both")]
    Both,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Debug, Clone)]
pub enum QueryExpressionFilter {
#[serde(rename = "any")]
    Any,
#[serde(rename = "null")]
    Null,
#[serde(rename = "wt-overexpressed")]
    WtOverexpressed,
}

type TermName = String;
type QueryRowsResult = Result<Vec<ResultRow>, String>;
type GeneUniquenameVecResult = Result<Vec<GeneUniquename>, String>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum QueryNode {
#[serde(rename = "or")]
    Or(Vec<QueryNode>),
#[serde(rename = "and")]
    And(Vec<QueryNode>),
#[serde(rename = "not")]
    Not { node_a: Box<QueryNode>, node_b: Box<QueryNode> },
#[serde(rename = "term")]
    Term {
        termid: String,
        name: Option<TermName>,
        single_or_multi_allele: Option<SingleOrMultiAllele>,
        expression: Option<QueryExpressionFilter>,
    },
#[serde(rename = "subset")]
    Subset { subset_name: String },
#[serde(rename = "gene_list")]
    GeneList { ids: Vec<GeneUniquename> },
#[serde(rename = "int_range")]
    IntRange { range_type: IntRangeType, start: Option<u64>, end: Option<u64> },
#[serde(rename = "float_range")]
    FloatRange { range_type: FloatRangeType, start: Option<f64>, end: Option<f64> },
}

fn exec_or(server_data: &ServerData, nodes: &Vec<QueryNode>) -> GeneUniquenameVecResult {
    if nodes.len() == 0 {
        return Err("illegal query: OR operator has no nodes".into());
    }

    let mut seen_genes = HashSet::new();
    let mut or_rows = vec![];

    for node in nodes {
        let exec_rows = node.exec(server_data)?;

        for row_gene_uniquename in &exec_rows {
            if !seen_genes.contains(row_gene_uniquename) {
                or_rows.push(row_gene_uniquename.clone());
                seen_genes.insert(row_gene_uniquename.clone());
            }
        }
    }

    Ok(or_rows)
}

fn exec_and(server_data: &ServerData, nodes: &Vec<QueryNode>) -> GeneUniquenameVecResult {
    if nodes.len() == 0 {
        return Err("illegal query: AND operator has no nodes".into());
    }

    let first_node_genes = nodes[0].exec(server_data)?;

    let current_genes = first_node_genes;

    let mut current_gene_set = HashSet::from_iter(current_genes);

    for node in nodes[1..].iter() {
        let node_result_rows = node.exec(server_data)?;
        let node_genes = node_result_rows.into_iter().collect::<HashSet<_>>();

        current_gene_set = current_gene_set.intersection(&node_genes).cloned().collect();
    }

    Ok(current_gene_set.into_iter().collect())
}

fn exec_not(server_data: &ServerData, node_a: &QueryNode, node_b: &QueryNode)
             -> GeneUniquenameVecResult
{
    let node_b_result = node_b.exec(server_data)?;

    let node_b_gene_set: HashSet<GeneUniquename> =
        HashSet::from_iter(node_b_result.into_iter());

    let node_a_result = node_a.exec(server_data)?;

    let mut not_rows = vec![];

    for row_gene_uniquename in &node_a_result {
        if !node_b_gene_set.contains(row_gene_uniquename) {
            not_rows.push(row_gene_uniquename.clone());
        }
    }

    Ok(not_rows)
}

fn exec_termid(server_data: &ServerData, term_id: &str,
               maybe_single_or_multi_allele: &Option<SingleOrMultiAllele>,
               expression: &Option<QueryExpressionFilter>)  -> GeneUniquenameVecResult {
    if let Some(ref single_or_multi_allele) = *maybe_single_or_multi_allele {
        let genes = server_data.genes_of_genotypes(term_id, single_or_multi_allele,
                                                   expression);
        Ok(genes)
    } else {
        Ok(server_data.genes_of_termid(term_id))
    }
}

fn exec_subset(server_data: &ServerData, subset_name: &str)  -> GeneUniquenameVecResult {
    Ok(server_data.genes_of_subset(subset_name))
}

fn exec_gene_list(gene_uniquenames: &Vec<GeneUniquename>)  -> GeneUniquenameVecResult {
    Ok(gene_uniquenames.clone())
}

fn exec_genome_range_overlaps(server_data: &ServerData,
                              range_start: Option<u64>, range_end: Option<u64>)
                               -> GeneUniquenameVecResult
{
    let gene_uniquenames =
        server_data.filter_genes(&|gene: &APIGeneSummary| {
            if let Some(ref location) = gene.location {
                (range_end.is_none() || location.start_pos as u64 <= range_end.unwrap()) &&
                (range_start.is_none() || location.end_pos as u64 >= range_start.unwrap())
            } else {
                false
            }
        });
    Ok(gene_uniquenames)
}

fn exec_protein_length_range(server_data: &ServerData,
                             range_start: Option<u64>, range_end: Option<u64>)
                              -> GeneUniquenameVecResult
{
    let gene_uniquenames =
        server_data.filter_genes(&|gene: &APIGeneSummary| {
            if gene.transcripts.len() > 0 {
                if let Some(ref protein) = gene.transcripts[0].protein {
                    (range_start.is_none() || protein.sequence.len() as u64 >= range_start.unwrap()) &&
                    (range_end.is_none() || protein.sequence.len() as u64 <= range_end.unwrap())
                } else {
                    false
                }
            } else {
                false
            }
        });
    Ok(gene_uniquenames)
}

fn exec_tm_domain_count_range(server_data: &ServerData,
                              range_start: Option<u64>, range_end: Option<u64>)
                               -> GeneUniquenameVecResult
{
    let gene_uniquenames =
        server_data.filter_genes(&|gene: &APIGeneSummary| {
            (range_start.is_none() || gene.tm_domain_count as u64 >= range_start.unwrap()) &&
            (range_end.is_none() || gene.tm_domain_count as u64 <= range_end.unwrap())
        });
    Ok(gene_uniquenames)
}

fn exec_exon_count_range(server_data: &ServerData,
                         range_start: Option<u64>, range_end: Option<u64>)
                         -> GeneUniquenameVecResult
{
    let gene_uniquenames =
        server_data.filter_genes(&|gene: &APIGeneSummary| {
            (range_start.is_none() || gene.exon_count as u64 >= range_start.unwrap()) &&
            (range_end.is_none() || gene.exon_count as u64 <= range_end.unwrap())
        });
    Ok(gene_uniquenames)
}

fn exec_int_range(server_data: &ServerData, range_type: &IntRangeType,
                  start: Option<u64>, end: Option<u64>) -> GeneUniquenameVecResult {
    match *range_type {
        IntRangeType::GenomeRangeContains => exec_genome_range_overlaps(server_data, start, end),
        IntRangeType::ProteinLength => exec_protein_length_range(server_data, start, end),
        IntRangeType::TMDomainCount => exec_tm_domain_count_range(server_data, start, end),
        IntRangeType::ExonCount => exec_exon_count_range(server_data, start, end),
    }
}

fn exec_mol_weight_range(server_data: &ServerData, range_start: Option<f64>, range_end: Option<f64>)
                         -> GeneUniquenameVecResult
{
    let gene_uniquenames =
        server_data.filter_genes(&|gene: &APIGeneSummary| {
            if gene.transcripts.len() > 0 {
                if let Some(ref protein) = gene.transcripts[0].protein {
                    (range_start.is_none() ||
                        protein.molecular_weight as f64 >= range_start.unwrap()) &&
                    (range_end.is_none() ||
                        protein.molecular_weight as f64 <= range_end.unwrap())
                } else {
                    false
                }
            } else {
                false
            }
        });
    Ok(gene_uniquenames)
}

fn exec_float_range(server_data: &ServerData, range_type: &FloatRangeType,
                    start: Option<f64>, end: Option<f64>) -> GeneUniquenameVecResult {
    match *range_type {
        FloatRangeType::ProteinMolWeight => exec_mol_weight_range(server_data, start, end)
    }
}

impl QueryNode {
    pub fn exec(&self, server_data: &ServerData) -> GeneUniquenameVecResult {
        use self::QueryNode::*;
        match *self {
            Or(ref nodes) => exec_or(server_data, nodes),
            And(ref nodes) => exec_and(server_data, nodes),
            Not { ref node_a, ref node_b } => exec_not(server_data, node_a, node_b),
            Term {
                ref termid,
                name: _,
                ref single_or_multi_allele,
                ref expression,
            } => exec_termid(server_data, termid, single_or_multi_allele, expression),
            Subset { ref subset_name } => exec_subset(server_data, subset_name),
            GeneList { ref ids } => exec_gene_list(ids),
            IntRange { ref range_type, start, end } =>
                exec_int_range(server_data, range_type, start, end),
            FloatRange { ref range_type, start, end } =>
                exec_float_range(server_data, range_type, start, end),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SeqType {
#[serde(rename = "protein")]
    Protein,
#[serde(rename = "nucleotide")]
    Nucleotide {
        include_introns: bool,
        include_5_prime_utr: bool,
        include_3_prime_utr: bool,
    },
#[serde(rename = "none")]
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QueryOutputOptions {
    pub sequence: SeqType,
    pub field_names: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Query {
    output_options: QueryOutputOptions,
    constraints: QueryNode,
}

impl Query {
    pub fn new(constraints: QueryNode, output_options: QueryOutputOptions) -> Query {
        Query {
            output_options: output_options,
            constraints: constraints
        }
    }

    fn make_result_rows(&self, genes: Vec<String>) -> QueryRowsResult {
        Ok(genes.into_iter()
           .map(|gene_uniquename| ResultRow {
               sequence: None,
               gene_uniquename: gene_uniquename,
           }).collect::<Vec<_>>())
    }

    pub fn exec(&self, server_data: &ServerData) -> QueryRowsResult {
        let genes_result = self.constraints.exec(server_data);

        match genes_result {
            Ok(genes) => self.make_result_rows(genes),
            Err(err) => Err(err)
        }
    }
}
