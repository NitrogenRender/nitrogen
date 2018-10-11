type SamplerGeneration = u64;
type SamplerId = usize;

#[derive(Copy, Clone)]
pub struct SamplerHandle(SamplerId, SamplerGeneration);


