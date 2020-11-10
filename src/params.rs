use crate::NonNativeFieldParams;
use ark_ff::PrimeField;
use ark_relations::r1cs::ConstraintSystemRef;
use ark_std::{
    any::{Any, TypeId},
    boxed::Box,
    collections::BTreeMap,
};

/// The type for a cache map for parameters
pub type ParamsMap = BTreeMap<(usize, usize), NonNativeFieldParams>;
#[derive(Clone)]
/// Statistics for hit rate of cache
pub struct HitRate {
    /// Number of hits
    hit: usize,
    /// Number of misses
    miss: usize,
}

impl HitRate {
    /// Initialize and activate the statistics
    pub fn init<BaseField: PrimeField>(cs: &ConstraintSystemRef<BaseField>) {
        match cs {
            ConstraintSystemRef::None => (),
            ConstraintSystemRef::CS(v) => {
                let cs_sys = v.borrow_mut();
                let mut big_map = cs_sys.cache_map.borrow_mut();
                big_map.insert(
                    TypeId::of::<HitRate>(),
                    Box::new(HitRate { hit: 0, miss: 0 }),
                );
            }
        }
    }

    /// Add to the statistics
    pub fn update(pmap: &mut BTreeMap<TypeId, Box<dyn Any>>, hit: bool) {
        let hit_rate = pmap.get(&TypeId::of::<HitRate>());

        if let Some(stat) = hit_rate.and_then(|rate| rate.downcast_ref::<HitRate>()) {
            let mut hit_rate = (*stat).clone();
            if hit {
                hit_rate.hit += 1;
            } else {
                hit_rate.miss += 1;
            }
            pmap.insert(TypeId::of::<HitRate>(), Box::new(hit_rate));
        }
    }

    /// Print out the statistics
    #[cfg(feature = "std")]
    pub fn print<BaseField: PrimeField>(cs: &ConstraintSystemRef<BaseField>) {
        match cs {
            ConstraintSystemRef::None => (),
            ConstraintSystemRef::CS(v) => {
                let cs_sys = v.borrow();
                let big_map = cs_sys.cache_map.borrow();
                let hit_rate = big_map.get(&TypeId::of::<HitRate>());

                if hit_rate.is_some() {
                    match hit_rate.unwrap().downcast_ref::<HitRate>() {
                        Some(stat) => {
                            let hit_rate = (*stat).clone();
                            println!(
                                "Hit: {}, Miss: {}, Hit Rate = {}",
                                hit_rate.hit,
                                hit_rate.miss,
                                (hit_rate.hit as f64) / ((hit_rate.hit + hit_rate.miss) as f64)
                            );
                        }
                        None => (),
                    }
                }
            }
        }
    }
}

/// Obtain the parameters from a `ConstraintSystem`'s cache or generate a new one
#[must_use]
pub fn get_params<TargetField: PrimeField, BaseField: PrimeField>(
    cs: &ConstraintSystemRef<BaseField>,
) -> NonNativeFieldParams {
    match cs {
        ConstraintSystemRef::None => gen_params::<TargetField, BaseField>(),
        ConstraintSystemRef::CS(v) => {
            let cs_sys = v.borrow_mut();
            let mut big_map = cs_sys.cache_map.borrow_mut();
            let small_map = big_map.get(&TypeId::of::<ParamsMap>());

            if let Some(small_map) = small_map {
                if let Some(map) = small_map.downcast_ref::<ParamsMap>() {
                    let params = map.get(&(BaseField::size_in_bits(), TargetField::size_in_bits()));
                    if let Some(params) = params {
                        let params = params.clone();
                        HitRate::update(&mut *big_map, true);
                        params
                    } else {
                        let params = gen_params::<TargetField, BaseField>();

                        let mut small_map = (*map).clone();
                        small_map.insert(
                            (BaseField::size_in_bits(), TargetField::size_in_bits()),
                            params.clone(),
                        );
                        big_map.insert(TypeId::of::<ParamsMap>(), Box::new(small_map));

                        HitRate::update(&mut *big_map, false);
                        params
                    }
                } else {
                    let params = gen_params::<TargetField, BaseField>();

                    let mut small_map = ParamsMap::new();
                    small_map.insert(
                        (BaseField::size_in_bits(), TargetField::size_in_bits()),
                        params.clone(),
                    );

                    big_map.insert(TypeId::of::<ParamsMap>(), Box::new(small_map));
                    HitRate::update(&mut *big_map, false);
                    params
                }
            } else {
                let params = gen_params::<TargetField, BaseField>();

                let mut small_map = ParamsMap::new();
                small_map.insert(
                    (BaseField::size_in_bits(), TargetField::size_in_bits()),
                    params.clone(),
                );

                big_map.insert(TypeId::of::<ParamsMap>(), Box::new(small_map));
                HitRate::update(&mut *big_map, false);
                params
            }
        }
    }
}

/// Generate the new params
#[must_use]
pub fn gen_params<TargetField: PrimeField, BaseField: PrimeField>() -> NonNativeFieldParams {
    let optimization_type = if cfg!(feature = "density-optimized") {
        OptimizationType::Density
    } else {
        OptimizationType::Constraints
    };

    let mut problem = ParamsSearching::new(
        BaseField::size_in_bits(),
        TargetField::size_in_bits(),
        optimization_type,
    );
    problem.solve();

    NonNativeFieldParams {
        num_limbs: problem.num_of_limbs.unwrap(),
        bits_per_limb: problem.limb_size.unwrap(),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
/// The type of optimization target for the parameters searching
pub enum OptimizationType {
    /// Optimized for constraints
    Constraints,
    /// Optimized for density
    Density,
}

/// A search instance for parameters for nonnative field gadgets
#[derive(Clone, Debug)]
pub struct ParamsSearching {
    // Problem
    /// Prime length of the base field
    pub base_field_prime_length: usize,
    /// Prime length of the target field
    pub target_field_prime_bit_length: usize,
    /// Constraints or density
    pub optimization_type: OptimizationType,

    // Solution
    /// Number of limbs
    pub num_of_limbs: Option<usize>,
    /// Size of the limb
    pub limb_size: Option<usize>,
}

impl ParamsSearching {
    /// Create the search problem
    #[must_use]
    pub fn new(
        base_field_prime_length: usize,
        target_field_prime_bit_length: usize,
        optimization_type: OptimizationType,
    ) -> Self {
        Self {
            base_field_prime_length,
            target_field_prime_bit_length,
            optimization_type,
            num_of_limbs: None,
            limb_size: None,
        }
    }

    /// Solve the search problem
    pub fn solve(&mut self) {
        let mut min_cost: Option<usize> = None;
        let mut min_cost_limb_size: Option<usize> = None;
        let mut min_cost_num_of_limbs: Option<usize> = None;

        let surfeit = 10;

        let max_limb_size = (self.base_field_prime_length - 1 - surfeit - 1) / 2;

        for limb_size in 1..=max_limb_size {
            let num_of_limbs = (self.target_field_prime_bit_length + limb_size - 1) / limb_size;

            let group_size = (self.base_field_prime_length - 1 - surfeit - 1) / (2 * limb_size);
            let num_of_groups = (2 * num_of_limbs - 1 + group_size - 1) / group_size;

            let mut this_cost = 0;

            if self.optimization_type == OptimizationType::Constraints {
                this_cost += 2 * num_of_limbs - 1;
            } else {
                this_cost += num_of_limbs * num_of_limbs / 2;
            }

            if self.optimization_type == OptimizationType::Constraints {
                this_cost +=
                    num_of_groups + (num_of_groups - 1) * (limb_size * 2 + 1 + 2 * surfeit) + 1;
            } else {
                this_cost +=
                    3 * num_of_groups + (num_of_groups - 1) * (limb_size * 2 + 1 + 2 * surfeit) + 2;
            }

            if min_cost == None || this_cost < min_cost.unwrap() {
                min_cost = Some(this_cost);
                min_cost_limb_size = Some(limb_size);
                min_cost_num_of_limbs = Some(num_of_limbs);
            }
        }

        self.num_of_limbs = min_cost_num_of_limbs;
        self.limb_size = min_cost_limb_size;
    }
}
