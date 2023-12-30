//! Initial points, is there general code that can be used to create starting points for the algorithms you wirte.
//!
//! Provides a set of initial points that can be used to start the algorithms, these include:
//! - Random points
//! - Central points
//! - Alpha points
//! - Rho points
//! - Random binary points

use crate::qubo::Qubo;
use ndarray::Array1;
use smolprng::{Algorithm, PRNG};

/// Generates a vector of random starting points, that is binary, meaning that it will return an array of floats
pub fn generate_random_starting_point<T: Algorithm>(
    qubo: &Qubo,
    prng: &mut PRNG<T>,
) -> Array1<f64> {
    let mut x = Array1::<f64>::zeros(qubo.num_x());

    for i in 0..x.len() {
        x[i] = prng.gen_f64();
    }
    x
}

/// Generates a vector of random starting points, that is fractional, meaning that it will return a vector of arrays
/// that are not just 0.0 or 1.0, but also numbers between.
pub fn generate_random_starting_points<T: Algorithm>(
    qubo: &Qubo,
    num_points: usize,
    prng: &mut PRNG<T>,
) -> Vec<Array1<f64>> {
    (0..num_points)
        .iter()
        .map(|_| generate_random_starting_point(qubo, prng))
        .collect()
}

/// Generates a starting point of exactly 0.5 for each variable.
pub fn generate_central_starting_points(qubo: &Qubo) -> Array1<f64> {
    Array1::<f64>::zeros(qubo.num_x()) + 0.5f64
}

/// Generates a starting poing based on the alpha heuristic, from the paper boros2007, which is the optimal solution to
/// the relaxed QUBO, given x_i = x_j for all i, j.
pub fn generate_alpha_starting_point(qubo: &Qubo) -> Array1<f64> {
    Array1::<f64>::zeros(qubo.num_x()) + qubo.alpha()
}

/// Generates a starting poing based on the rho heuristic, from the paper boros2007, which is the optimal solution to
/// the relaxed QUBO, given x_i = x_j for all i, j.
pub fn generate_rho_starting_point(qubo: &Qubo) -> Array1<f64> {
    Array1::<f64>::zeros(qubo.num_x()) + qubo.rho()
}

/// Generates a random binary point, where each variable has a probability of being 1.0 equal to sparsity.
pub fn generate_random_binary_point<T: Algorithm>(
    qubo: &Qubo,
    prng: &mut PRNG<T>,
    sparsity: f64,
) -> Array1<f64> {
    let mut x = Array1::<f64>::zeros(qubo.num_x());
    for i in 0..x.len() {
        if prng.gen_f64() < sparsity {
            x[i] = 1.0;
        }
    }
    x
}
