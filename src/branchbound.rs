use crate::qubo::Qubo;
use ndarray::Array1;
use std::collections::HashMap;
use std::time;

use crate::local_search_utils;
use crate::persistence::compute_iterative_persistence;
use clarabel::algebra::*;
use clarabel::solver::*;
use smolprng::{JsfLarge, PRNG};
use sprs::{CsMat, TriMat};

/// Bare bones implementation of B&B. Currently requires the QUBO to be symmetrical and convex.
/// Currently, the deterministic solver is solved via Clarabel.rs.

/// Struct the describes the branch and bound tree nodes
#[derive(Clone)]
pub struct QuboBBNode {
    pub lower_bound: f64,
    pub solution: Array1<f64>,
    pub fixed_variables: HashMap<usize, f64>,
}

/// Struct for the B&B Solver
pub struct BBSolver {
    pub qubo: Qubo,
    pub best_solution: Array1<f64>,
    pub best_solution_value: f64,
    pub nodes: Vec<QuboBBNode>,
    pub nodes_processed: usize,
    pub nodes_visited: usize,
    pub nodes_objective_evales: usize,
    pub time_start: f64,
    pub clarabel_wrapper: ClarabelWrapper,
    pub options: SolverOptions,
}

/// Options for the B&B solver for run time
pub struct SolverOptions {
    pub branch_strategy: BranchStrategy,
    pub max_time: f64,
    pub seed: usize,
}

pub enum BranchStrategy {
    FirstNotFixed,
    MostViolated,
    Random,
    WorstApproximation,
}

/// Wrapper to help convert the QUBO to the format required by Clarabel.rs
pub struct ClarabelWrapper {
    pub q: CscMatrix,
    pub c: Array1<f64>,
}

impl ClarabelWrapper {
    pub fn new(qubo: &Qubo) -> ClarabelWrapper {
        let q_new = ClarabelWrapper::make_cb_form(&(qubo.q));
        ClarabelWrapper {
            q: q_new,
            c: qubo.c.clone(),
        }
    }

    pub fn make_cb_form(p0: &CsMat<f64>) -> CscMatrix {
        let (t, y, u) = p0.to_csc().into_raw_storage();
        CscMatrix::new(p0.rows(), p0.cols(), t, y, u)
    }
}

impl BBSolver {
    /// Creates a new B&B solver
    pub fn new(qubo: Qubo, options: SolverOptions) -> BBSolver {
        // create auxiliary variables
        let num_x = qubo.num_x();
        let wrapper = ClarabelWrapper::new(&qubo);

        BBSolver {
            qubo,
            best_solution: Array1::zeros(num_x),
            best_solution_value: 0.0,
            nodes: Vec::new(),
            nodes_processed: 0,
            nodes_visited: 0,
            nodes_objective_evales: 0,
            time_start: 0.0,
            clarabel_wrapper: wrapper,
            options,
        }
    }

    /// This function is used to warm start the solver with an initial solution if one is not provided
    pub fn warm_start(&mut self, initial_solution: Array1<f64>) {
        self.best_solution = initial_solution;
        self.best_solution_value = self.qubo.eval(&self.best_solution);
    }

    /// The main solve function of the B&B algorithm
    pub fn solve(&mut self) -> (Array1<f64>, f64) {
        // compute an initial set of persistent variables
        let initial_persistent =
            compute_iterative_persistence(&self.qubo, &HashMap::new(), self.qubo.num_x());

        // create the root node
        let root_node = QuboBBNode {
            lower_bound: f64::NEG_INFINITY,
            solution: Array1::zeros(self.qubo.num_x()),
            fixed_variables: initial_persistent,
        };
        // add the root node to the list of nodes
        self.nodes.push(root_node);

        // set the start time
        self.time_start = time::SystemTime::now()
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        // until we have hit a termination condition, we will keep iterating
        while !(*self).termination_condition() {
            // get next node, if it exists
            let next_node = self.get_next_node();

            // there are no more nodes to process, so we are done iterating
            if next_node.is_none() {
                break;
            }

            // unwrap the node
            let node = next_node.unwrap();

            // as we are processing the node we increment the number of nodes processed
            self.nodes_processed += 1;

            // We now need to solve the node to generate the lower bound and solution
            let (lower_bound, solution) = self.solve_node(&node);

            // determine what variable we are branching on
            let branch_id = self.make_branch(&node);

            // generate the branches
            let (zero_branch, one_branch) = self.branch(node, branch_id, lower_bound, solution);

            // add the branches to the list of nodes
            self.nodes.push(zero_branch);
            self.nodes.push(one_branch);
        }

        return (self.best_solution.clone(), self.best_solution_value);
    }

    /// Checks if we can prune the node, based on the lower bound and best solution
    pub fn can_prune(&mut self, node: &QuboBBNode) -> bool {
        // if the lower bound is greater than the best solution, then we can prune
        if node.lower_bound > self.best_solution_value {
            return true;
        }

        // if the solution is complete, then we can update the best solution if better
        // we can also prune the node, as there are no more variables to fix
        if node.fixed_variables.len() == self.qubo.num_x() {
            // generate the solution vector
            let mut solution = Array1::zeros(self.qubo.num_x());
            for (&index, &value) in node.fixed_variables.iter() {
                solution[index] = value;
            }

            // evaluate the solution against the best solution we have so far
            // if we have a better solution update it
            let solution_value = self.qubo.eval(&solution);
            if solution_value < self.best_solution_value {
                self.best_solution = solution;
                self.best_solution_value = solution_value;
                self.nodes_objective_evales += 1;
            }
            return true;
        }

        // if we can not remove the node, then we return false as we cannot provably prune it yet
        return false;
    }

    /// This function is used to get the next node to process, popping it from the list of nodes
    pub fn get_next_node(&mut self) -> Option<QuboBBNode> {
        while self.nodes.len() > 0 {
            // we pull a node from our node list
            let optional_node = self.nodes.pop();

            // guard against the case where another thread might have popped the last node between the
            // check and the pop
            if optional_node.is_none() {
                return None;
            }

            // unwrap the node if it is safe
            let node = optional_node.unwrap();

            // we increment the number of nodes we have visited
            self.nodes_visited += 1;

            // if we can't prune it, then we return it
            if !self.can_prune(&node) {
                return Some(node);
            }
        }
        return None;
    }

    /// Checks for termination conditions of the B&B algorithm, such as time limit or no more nodes
    pub fn termination_condition(&self) -> bool {
        // get current time to check if we have exceeded the maximum time
        let current_time = time::SystemTime::now()
            .duration_since(time::SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();

        // check if we violated the time limit
        if current_time - self.time_start > self.options.max_time {
            return true;
        }

        // check if we have no more nodes to process
        if self.nodes.len() == 0 {
            return true;
        }

        return false;
    }

    /// Branch Selection Strategy - Currently selects the first variable that is not fixed
    pub fn make_branch(&mut self, node: &QuboBBNode) -> usize {
        return match self.options.branch_strategy {
            BranchStrategy::FirstNotFixed => self.first_not_fixed(node),
            BranchStrategy::MostViolated => self.most_violated(node),
            BranchStrategy::Random => self.random(node),
            BranchStrategy::WorstApproximation => self.worst_approximation(node),
        };
    }

    pub fn first_not_fixed(&self, node: &QuboBBNode) -> usize {
        // scan thru the variables and find the first one that is not fixed
        for i in 0..self.qubo.num_x() {
            if !node.fixed_variables.contains_key(&i) {
                return i;
            }
        }
        panic!("No variable to branch on");
    }

    pub fn most_violated(&self, node: &QuboBBNode) -> usize {
        let mut most_violated = 1.0;
        let mut index_most_violated = 0;

        for i in 0..self.qubo.num_x() {
            if !node.fixed_variables.contains_key(&i) {
                let violation = (node.solution[i] - 0.5).abs();

                if violation <= most_violated {
                    most_violated = violation;
                    index_most_violated = i;
                }
            }
        }

        index_most_violated
    }

    pub fn random(&self, node: &QuboBBNode) -> usize {
        // generate a prng
        let mut prng = PRNG {
            generator: JsfLarge::from(self.options.seed as u64 + self.nodes_visited as u64),
        };

        // generate a random index in the list of variables
        let index = (prng.gen_u64() % self.qubo.num_x() as u64) as usize;

        // scan thru the variables and find the first one that is not fixed starting at the random point
        for i in index..self.qubo.num_x() {
            if !node.fixed_variables.contains_key(&i) {
                return i;
            }
        }

        // scane thru the variables and find the first one that is not fixed starting at the beginning
        for i in 0..index {
            if !node.fixed_variables.contains_key(&i) {
                return i;
            }
        }

        panic!("No Variable to branch on")
    }

    /// Branches on the variable that has an estimated worst result, pushing up the lower bound as fast as possible
    pub fn worst_approximation(&mut self, node: &QuboBBNode) -> usize {
        let mut zero_buffer = node.solution.clone();
        let mut one_buffer = node.solution.clone();

        // fix all the nonfixed variables
        for i in 0..self.qubo.num_x() {
            if node.fixed_variables.contains_key(&i) {
                zero_buffer[i] = *node.fixed_variables.get(&i).unwrap();
                one_buffer[i] = *node.fixed_variables.get(&i).unwrap();
            } else {
                zero_buffer[i] = 0.0;
                one_buffer[i] = 1.0;
            }
        }

        // find the objective change when flipping each variable to either 0 or 1
        let (_, flips_zero) = local_search_utils::one_flip_objective(&self.qubo, &zero_buffer);
        let (_, flips_one) = local_search_utils::one_flip_objective(&self.qubo, &one_buffer);

        // counts as effectively 4 objective evaluate
        self.nodes_objective_evales += 4;

        // tracking variables for the worst approximation
        let mut worst_approximation = f64::NEG_INFINITY;
        let mut index_worst_approximation = 0;

        // scan thru the variables and find the worst gain
        for i in 0..self.qubo.num_x() {
            // if it is a fixed node, then skip it
            if node.fixed_variables.contains_key(&i) {
                continue;
            }

            // find the minimum of the two objective changes
            let min_obj_gain = flips_zero[i].min(flips_one[i]);

            // if it is the highest growing variable, then update the tracking variables
            if min_obj_gain > worst_approximation {
                worst_approximation = min_obj_gain;
                index_worst_approximation = i;
            }
        }

        index_worst_approximation
    }

    /// Actually branches the node into two new nodes
    pub fn branch(
        &self,
        node: QuboBBNode,
        branch_id: usize,
        lower_bound: f64,
        solution: Array1<f64>,
    ) -> (QuboBBNode, QuboBBNode) {
        // make two new nodes, one with the variable set to 0 and the other set to 1
        let mut zero_branch = node.clone();
        let mut one_branch = node;

        // add fixed variables
        zero_branch.fixed_variables.insert(branch_id, 0.0);
        one_branch.fixed_variables.insert(branch_id, 1.0);

        // apply iterative persistence to the fixed variables every time we branch
        zero_branch.fixed_variables = compute_iterative_persistence(
            &self.qubo,
            &zero_branch.fixed_variables,
            self.qubo.num_x(),
        );
        one_branch.fixed_variables = compute_iterative_persistence(
            &self.qubo,
            &one_branch.fixed_variables,
            self.qubo.num_x(),
        );

        // update the solution and lower bound for the new nodes
        zero_branch.solution = solution.clone();
        one_branch.solution = solution;

        // set the lower bound for the new nodes
        zero_branch.lower_bound = lower_bound;
        one_branch.lower_bound = lower_bound;

        return (zero_branch, one_branch);
    }

    /// Solves the subproblem via a QP solver, in this case Clarabel.rs
    pub fn solve_node(&self, node: &QuboBBNode) -> (f64, Array1<f64>) {
        // solve QP associated with the node

        // generate default settings
        let mut settings = DefaultSettings::default();
        settings.verbose = false;

        // generate the constraint matrix
        let A_size = 2 * self.qubo.num_x() + node.fixed_variables.len();
        let mut A = TriMat::new((A_size, self.qubo.num_x()));
        let mut b = Array1::zeros(A_size);

        // add the equality constraints
        for (index, (&key, &value)) in node.fixed_variables.iter().enumerate() {
            A.add_triplet(index, key, 1.0);
            b[index] = value;
        }

        // add the inequality constraints
        for (index, i) in (0..self.qubo.num_x()).enumerate() {
            let offset = node.fixed_variables.len() + index * 2;
            A.add_triplet(offset, i, 1.0);
            A.add_triplet(offset + 1, i, -1.0);
            b[offset] = 1.0;
            b[offset + 1] = 0.0;
        }

        // convert the matrix to CSC format and then Clarabel format
        let A_csc = A.to_csc();
        let A_clara = ClarabelWrapper::make_cb_form(&A_csc);

        // generate the cones for the solver
        let cones = [
            ZeroConeT(node.fixed_variables.len()),
            NonnegativeConeT(2 * self.qubo.num_x()),
        ];

        // set up the solver with the matrices
        let mut solver = DefaultSolver::new(
            &self.clarabel_wrapper.q,
            &self.qubo.c.as_slice().unwrap(),
            &A_clara,
            &b.as_slice().unwrap(),
            &cones,
            settings,
        );

        // actually solve the problem
        solver.solve();

        return (solver.solution.obj_val, Array1::from(solver.solution.x));
    }
}
