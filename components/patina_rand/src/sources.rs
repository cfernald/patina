use patina::component::service::random::{RandomAlgorithm, RandomError};

#[cfg(target_arch = "x86_64")]
mod rdrand;
#[cfg(target_arch = "aarch64")]
mod rndr;

pub(crate) const SOURCES: &[&dyn RandomSource] = &[];

pub(crate) trait RandomSource {
    fn available_algorithm(&self) -> Option<RandomAlgorithm>;

    fn get_word(&self) -> Result<u64, RandomError>;

    fn fill_bytes(&self, output: &mut [u8]) -> Result<(), RandomError> {
        for chunk in output.chunks_mut(8) {
            let word = self.get_word()?;
            let bytes = word.to_ne_bytes();
            for (i, byte) in chunk.iter_mut().enumerate() {
                *byte = bytes[i];
            }
        }
        Ok(())
    }
}
