use std::{
    collections::HashMap,
    intrinsics::transmute,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

use rayon::iter::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator};

use crate::{
    input::{Direction, DotGraph, EdgeList},
    DirectedGraph, Graph, UndirectedGraph,
};

pub struct CSR {
    offsets: Box<[usize]>,
    targets: Box<[usize]>,
}

impl CSR {
    #[inline]
    fn node_count(&self) -> usize {
        self.offsets.len() - 1
    }

    #[inline]
    fn edge_count(&self) -> usize {
        self.targets.len()
    }

    #[inline]
    fn degree(&self, node: usize) -> usize {
        self.offsets[node + 1] - self.offsets[node]
    }

    #[inline]
    fn neighbors(&self, node: usize) -> &[usize] {
        let from = self.offsets[node];
        let to = self.offsets[node + 1];
        &self.targets[from..to]
    }
}

impl From<(&EdgeList, usize, Direction)> for CSR {
    fn from((edge_list, node_count, direction): (&EdgeList, usize, Direction)) -> Self {
        let mut start = Instant::now();

        println!("Start: degrees()");
        let degrees = edge_list.degrees(node_count, direction);
        println!("Finish: degrees() took {} ms", start.elapsed().as_millis());
        start = Instant::now();

        println!("Start: prefix_sum()");
        let offsets = prefix_sum(&degrees);
        println!(
            "Finish: prefix_sum() took {} ms",
            start.elapsed().as_millis()
        );
        start = Instant::now();

        let targets = vec![0_usize; offsets[node_count]];

        let targets = unsafe { transmute::<_, Vec<AtomicUsize>>(targets) };
        let offsets = unsafe { transmute::<_, Vec<AtomicUsize>>(offsets) };

        println!("Start: targets");
        match direction {
            Direction::Outgoing => edge_list.par_iter().for_each(|(s, t)| {
                targets[offsets[*s].fetch_add(1, Ordering::SeqCst)].store(*t, Ordering::SeqCst);
            }),
            Direction::Incoming => edge_list.par_iter().for_each(|(s, t)| {
                targets[offsets[*t].fetch_add(1, Ordering::SeqCst)].store(*s, Ordering::SeqCst);
            }),
            Direction::Undirected => edge_list.par_iter().for_each(|(s, t)| {
                targets[offsets[*s].fetch_add(1, Ordering::SeqCst)].store(*t, Ordering::SeqCst);
                targets[offsets[*t].fetch_add(1, Ordering::SeqCst)].store(*s, Ordering::SeqCst);
            }),
        }
        println!("Finish: targets took {} ms", start.elapsed().as_millis());
        start = Instant::now();

        let mut offsets = unsafe { transmute::<_, Vec<usize>>(offsets) };
        let mut targets = unsafe { transmute::<_, Vec<usize>>(targets) };

        // the previous loop moves all offsets one index to the right
        // we need to correct this to have proper offsets
        offsets.pop();
        offsets.insert(0, 0);

        println!("Start: sort_targets()");
        sort_targets(&offsets, &mut targets);
        println!(
            "Finish: sort_targets() took {} ms",
            start.elapsed().as_millis()
        );

        CSR {
            offsets: offsets.into_boxed_slice(),
            targets: targets.into_boxed_slice(),
        }
    }
}

fn prefix_sum(degrees: &[usize]) -> Vec<usize> {
    let mut sums = vec![0; degrees.len() + 1];
    let mut total = 0;

    for (i, degree) in degrees.iter().enumerate() {
        sums[i] = total;
        total += degree;
    }

    sums[degrees.len()] = total;

    sums
}

fn sort_targets(offsets: &[usize], targets: &mut [usize]) {
    let node_count = offsets.len() - 1;
    let mut target_chunks = Vec::with_capacity(node_count);
    let mut tail = targets;
    let mut prev_offset = offsets[0];

    for &offset in &offsets[1..node_count] {
        let (list, remainder) = tail.split_at_mut(offset - prev_offset);
        target_chunks.push(list);
        tail = remainder;
        prev_offset = offset;
    }

    // do the actual sorting of individual target lists
    target_chunks
        .par_iter_mut()
        .for_each(|list| list.sort_unstable());
}

pub struct DirectedCSRGraph {
    node_count: usize,
    edge_count: usize,
    out_edges: CSR,
    in_edges: CSR,
}

impl DirectedCSRGraph {
    pub fn new(out_edges: CSR, in_edges: CSR) -> Self {
        Self {
            node_count: out_edges.node_count(),
            edge_count: out_edges.edge_count(),
            out_edges,
            in_edges,
        }
    }
}

impl Graph for DirectedCSRGraph {
    fn node_count(&self) -> usize {
        self.node_count
    }

    fn edge_count(&self) -> usize {
        self.edge_count
    }
}

impl DirectedGraph for DirectedCSRGraph {
    fn out_degree(&self, node: usize) -> usize {
        self.out_edges.degree(node)
    }

    fn out_neighbors(&self, node: usize) -> &[usize] {
        self.out_edges.neighbors(node)
    }

    fn in_degree(&self, node: usize) -> usize {
        self.in_edges.degree(node)
    }

    fn in_neighbors(&self, node: usize) -> &[usize] {
        self.in_edges.neighbors(node)
    }
}

impl From<EdgeList> for DirectedCSRGraph {
    fn from(edge_list: EdgeList) -> Self {
        let node_count = edge_list.max_node_id() + 1;
        let out_edges = CSR::from((&edge_list, node_count, Direction::Outgoing));
        let in_edges = CSR::from((&edge_list, node_count, Direction::Incoming));

        DirectedCSRGraph::new(out_edges, in_edges)
    }
}

pub struct UndirectedCSRGraph {
    node_count: usize,
    edge_count: usize,
    edges: CSR,
}

impl UndirectedCSRGraph {
    pub fn new(edges: CSR) -> Self {
        Self {
            node_count: edges.node_count(),
            edge_count: edges.edge_count() / 2,
            edges,
        }
    }
}

impl Graph for UndirectedCSRGraph {
    fn node_count(&self) -> usize {
        self.node_count
    }

    fn edge_count(&self) -> usize {
        self.edge_count
    }
}

impl UndirectedGraph for UndirectedCSRGraph {
    fn degree(&self, node: usize) -> usize {
        self.edges.degree(node)
    }

    fn neighbors(&self, node: usize) -> &[usize] {
        self.edges.neighbors(node)
    }
}

impl From<EdgeList> for UndirectedCSRGraph {
    fn from(edge_list: EdgeList) -> Self {
        let node_count = edge_list.max_node_id() + 1;
        let edges = CSR::from((&edge_list, node_count, Direction::Undirected));

        UndirectedCSRGraph::new(edges)
    }
}

pub struct NodeLabeledCSRGraph<G> {
    graph: G,
    label_index: Box<[usize]>,
    label_index_offsets: Box<[usize]>,
    max_degree: usize,
    max_label: usize,
    max_label_frequency: usize,
    label_frequency: HashMap<usize, usize>,
    neighbor_label_frequencies: Option<Box<[HashMap<usize, usize>]>>,
}

impl<G: Graph> Graph for NodeLabeledCSRGraph<G> {
    #[inline]
    fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    #[inline]
    fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }
}

impl<G: DirectedGraph> DirectedGraph for NodeLabeledCSRGraph<G> {
    fn out_degree(&self, node: usize) -> usize {
        self.graph.out_degree(node)
    }

    fn out_neighbors(&self, node: usize) -> &[usize] {
        self.graph.out_neighbors(node)
    }

    fn in_degree(&self, node: usize) -> usize {
        self.graph.in_degree(node)
    }

    fn in_neighbors(&self, node: usize) -> &[usize] {
        self.graph.in_neighbors(node)
    }
}

impl<G: UndirectedGraph> UndirectedGraph for NodeLabeledCSRGraph<G> {
    fn degree(&self, node: usize) -> usize {
        self.graph.degree(node)
    }

    fn neighbors(&self, node: usize) -> &[usize] {
        self.graph.neighbors(node)
    }
}

impl<G: From<EdgeList>> From<DotGraph> for NodeLabeledCSRGraph<G> {
    fn from(_: DotGraph) -> Self {
        todo!()
    }
}
