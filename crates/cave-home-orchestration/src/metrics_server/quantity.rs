//! The `resource.Quantity` slice metrics-server depends on.
//!
//! Integer-only CPU (`DecimalSI`) and memory (`BinarySI`) quantity formatting.
//!
//! metrics-server expresses every usage value as a Kubernetes
//! `resource.Quantity`: CPU as a `DecimalSI` quantity in nanocore precision
//! (`resource.NewScaledQuantity(nanocores, -9)`), memory as a `BinarySI`
//! quantity in bytes (`resource.NewQuantity(bytes, BinarySI)`). A `Quantity`'s
//! *canonical string* picks the largest suffix that keeps the mantissa an
//! integer — `250m`, `1500m`, `2`, `1n` for CPU; `128Mi`, `1536Mi`, `2Gi` for
//! memory. Clients like `kubectl top` then render a rounded-up whole-milli /
//! whole-MiB value for the table.
//!
//! This is a behavioural reimplementation of exactly that slice — the canonical
//! `DecimalSI` / `BinarySI` suffix selection and the round-up display values —
//! reproduced from the public `resource.Quantity` contract. It is integer-only
//! (no arbitrary-precision `inf.Dec`), which is all the metrics pipeline needs:
//! every input is a `u64` count of nanocores or bytes.

/// CPU `DecimalSI` scales, largest first: (nanocores-per-unit, suffix).
/// One core = 1e9 nanocores; milli = 1e6; micro = 1e3; nano = 1.
const CPU_SCALES: [(u64, &str); 4] = [
    (1_000_000_000, ""),
    (1_000_000, "m"),
    (1_000, "u"),
    (1, "n"),
];

/// Memory `BinarySI` scales, largest first: (bytes-per-unit, suffix).
const MEM_SCALES: [(u64, &str); 6] = [
    (1 << 60, "Ei"),
    (1 << 50, "Pi"),
    (1 << 40, "Ti"),
    (1 << 30, "Gi"),
    (1 << 20, "Mi"),
    (1 << 10, "Ki"),
];

const NANOS_PER_MILLI: u64 = 1_000_000;
const BYTES_PER_MIB: u64 = 1 << 20;

/// One resource quantity — a CPU value (nanocores) or a memory value (bytes).
///
/// The flavour is carried so the canonical string uses the right suffix family;
/// the stored magnitude is a plain `u64` count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quantity {
    /// nanocores (CPU) or bytes (memory).
    amount: u64,
    flavour: Flavour,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Flavour {
    /// CPU — rendered `DecimalSI` (nano/micro/milli/core).
    Cpu,
    /// Memory — rendered `BinarySI` (Ki/Mi/Gi/…).
    Memory,
}

impl Quantity {
    /// A CPU quantity from a raw nanocore count (`1e9` nanocores = 1 core).
    #[must_use]
    pub const fn from_cpu_nanocores(nanocores: u64) -> Self {
        Self {
            amount: nanocores,
            flavour: Flavour::Cpu,
        }
    }

    /// A memory quantity from a raw byte count.
    #[must_use]
    pub const fn from_bytes(bytes: u64) -> Self {
        Self {
            amount: bytes,
            flavour: Flavour::Memory,
        }
    }

    /// The raw magnitude — nanocores for CPU, bytes for memory.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.amount
    }

    /// Canonical `DecimalSI` string for a CPU quantity: the largest suffix in
    /// {core, m, u, n} whose unit divides the nanocore count evenly. `0` → `"0"`.
    ///
    /// (On a memory quantity this still renders via the CPU scales; callers use
    /// [`Self::to_mem_string`] for memory — the flavour guards which is correct.)
    #[must_use]
    pub fn to_cpu_string(self) -> String {
        canonical(self.amount, &CPU_SCALES)
    }

    /// Canonical `BinarySI` string for a memory quantity: the largest binary
    /// suffix that divides the byte count evenly, else the raw byte count.
    /// `0` → `"0"`.
    #[must_use]
    pub fn to_mem_string(self) -> String {
        canonical(self.amount, &MEM_SCALES)
    }

    /// Millicores, **rounded up** — kubectl's `Quantity.MilliValue()` for the
    /// `top` table. A non-zero sub-milli usage renders as at least `1m`.
    #[must_use]
    pub const fn milli_cpu(self) -> u64 {
        div_ceil(self.amount, NANOS_PER_MILLI)
    }

    /// Mebibytes, **rounded up** — the whole-MiB figure `kubectl top` prints.
    /// A non-empty working set never rounds down to `0`.
    #[must_use]
    pub const fn mebibytes(self) -> u64 {
        div_ceil(self.amount, BYTES_PER_MIB)
    }

    /// Add two quantities, saturating at `u64::MAX`. The API layer sums
    /// per-container usage into a pod total; saturation keeps a pathological
    /// counter from panicking the pipeline. The flavour of `self` is preserved.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self {
            amount: self.amount.saturating_add(other.amount),
            flavour: self.flavour,
        }
    }
}

/// Round-up integer division (`⌈n / d⌉`) without overflow for the ranges used
/// here. `d` is a non-zero scale constant.
const fn div_ceil(n: u64, d: u64) -> u64 {
    n / d + if n % d == 0 { 0 } else { 1 }
}

/// Pick the largest scale that divides `amount` evenly and emit `q<suffix>`.
/// Falls through to the raw count (no suffix) when nothing divides it; `0`
/// is the canonical zero.
fn canonical(amount: u64, scales: &[(u64, &str)]) -> String {
    if amount == 0 {
        return "0".to_string();
    }
    for &(unit, suffix) in scales {
        if amount >= unit && amount % unit == 0 {
            return format!("{}{suffix}", amount / unit);
        }
    }
    amount.to_string()
}

/// The `{cpu, memory}` resource pair the metrics API attaches to every node and
/// container usage record (a `corev1.ResourceList` restricted to the two keys
/// metrics-server populates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceList {
    /// CPU usage.
    pub cpu: Quantity,
    /// Memory (working-set) usage.
    pub memory: Quantity,
}

impl ResourceList {
    /// Pair a CPU and a memory quantity.
    #[must_use]
    pub const fn new(cpu: Quantity, memory: Quantity) -> Self {
        Self { cpu, memory }
    }

    /// The zero usage list — `0` CPU, `0` memory. The additive identity for
    /// pod aggregation.
    #[must_use]
    pub const fn zero() -> Self {
        Self {
            cpu: Quantity::from_cpu_nanocores(0),
            memory: Quantity::from_bytes(0),
        }
    }

    /// Component-wise saturating sum — used to fold container usage into a pod
    /// total.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self {
            cpu: self.cpu.saturating_add(other.cpu),
            memory: self.memory.saturating_add(other.memory),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_scales_pick_largest_integer_suffix() {
        assert_eq!(
            Quantity::from_cpu_nanocores(1_000_000_000).to_cpu_string(),
            "1"
        );
        assert_eq!(
            Quantity::from_cpu_nanocores(250_000_000).to_cpu_string(),
            "250m"
        );
        assert_eq!(Quantity::from_cpu_nanocores(5_000).to_cpu_string(), "5u");
        assert_eq!(Quantity::from_cpu_nanocores(7).to_cpu_string(), "7n");
    }

    #[test]
    fn memory_falls_through_to_raw_bytes_when_indivisible() {
        // 3 MiB + 1 byte is divisible by nothing → raw bytes.
        assert_eq!(
            Quantity::from_bytes(3 * 1024 * 1024 + 1).to_mem_string(),
            "3145729"
        );
    }

    #[test]
    fn div_ceil_rounds_up_only_on_remainder() {
        assert_eq!(div_ceil(0, 10), 0);
        assert_eq!(div_ceil(10, 10), 1);
        assert_eq!(div_ceil(11, 10), 2);
    }

    #[test]
    fn resource_list_zero_is_additive_identity() {
        let rl = ResourceList::new(Quantity::from_cpu_nanocores(123), Quantity::from_bytes(456));
        assert_eq!(ResourceList::zero().saturating_add(rl), rl);
    }

    #[test]
    fn saturating_add_clamps_at_u64_max() {
        let big = Quantity::from_bytes(u64::MAX);
        assert_eq!(big.saturating_add(Quantity::from_bytes(10)).raw(), u64::MAX);
    }
}
