pub mod dense_eig;
pub mod error;
pub mod factor;
pub mod fingerprint;
pub mod matmul;
pub mod size;
pub mod sparse_eig;
pub mod sparse_solve;
pub mod traits;
pub mod types;

pub use error::CoreError;
pub use fingerprint::Fingerprint;
pub use size::{
    checked_elems, shape_text, variable_too_large, ELEM_C64, ELEM_F64, ELEM_I128, F64_TWO_POW_64,
    MAX_ELEMENTS,
};
pub use traits::{
    decompose::{
        CholeskyDecomposable, Decomposable, EigenDecomposable, LuDecomposable, SvdDecomposable,
    },
    filter::Filter,
    transform::Transform,
};
pub use types::{
    CMatrix, CTensor3, CVector, IntClass, OrderingHint, OverflowMode, RMatrix, RVector, RoundMode,
    SparseMat, SparseVec, C64,
};
