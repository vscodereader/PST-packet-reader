//! 계정(타깃)별 무작위 댓글 분배 — 이슈 #98.
//!
//! UI 안내문("계정마다 다른 댓글이 무작위로 게시돼")과 동작을 일치시키기 위해,
//! 댓글 풀을 셔플해 타깃마다 **1개씩** 배정한다. 분배 책임을 프론트엔드에서
//! 백엔드로 옮긴 것으로(과거 `comment-jobs.ts`), 시드를 주입하면 결정적이라
//! 단위 테스트로 검증할 수 있다.
//!
//! RNG는 프론트의 `mulberry32`를 Rust로 1:1 포팅했다. **같은 시드면 TS와
//! 완전히 동일한 시퀀스**를 내므로, 기존 프론트 테스트의 기대 동작을 그대로
//! 옮길 수 있다. 프로덕션 호출은 벽시계(wall-clock) 시드로 매 실행마다 달라진다.

use std::time::{SystemTime, UNIX_EPOCH};

/// `mulberry32` — 작고 빠른 32비트 시드 PRNG. 같은 `seed`는 항상 같은 스트림을
/// 내므로 분배가 재현 가능하다. 반환 클로저는 호출할 때마다 `[0, 1)` 실수를 낸다.
///
/// TS 원본(`comment-jobs.ts`)을 비트 단위로 포팅했다. `Math.imul`은 32비트 곱의
/// 하위 비트 → `wrapping_mul`, `+ | 0`은 32비트 wrapping → `wrapping_add`,
/// `>>>`(부호 없는 시프트)는 `u32 >>`에 대응한다.
pub fn mulberry32(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6d2b_79f5);
        let mut t = (a ^ (a >> 15)).wrapping_mul(a | 1);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// 벽시계(밀리초)로 만든 프로덕션 시드. TS의 `Date.now()` 시드와 같은 의도다.
/// 하위 32비트만 쓰며(`mulberry32`가 어차피 `u32`로 받음), 시계가 비정상이면 0.
pub fn seed_from_clock() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

/// `items`의 **복사본**을 Fisher–Yates로 셔플한다. 주입된 `rng`만으로 결과
/// 순서가 완전히 결정된다. `i ∈ [1, len-1]`, `j ∈ [0, i]`가 항상 범위 안이라
/// 스왑은 매번 일어난다(편향 없는 순열).
fn shuffle<T: Clone>(items: &[T], rng: &mut impl FnMut() -> f64) -> Vec<T> {
    let mut out = items.to_vec();
    let len = out.len();
    if len < 2 {
        return out;
    }
    for i in (1..len).rev() {
        // rng() ∈ [0, 1) 이므로 j = floor(rng()*(i+1)) ∈ [0, i] — 범위 보장.
        let j = (rng() * (i as f64 + 1.0)) as usize;
        out.swap(i, j);
    }
    out
}

/// 댓글 풀을 셔플해 `count`개 타깃에 라운드로빈으로 **1개씩** 배정한다.
/// 반환 벡터의 `i`번째가 `i`번째 타깃에 달릴 댓글이다.
///
/// 경계 동작:
/// - **댓글 < 타깃**: 셔플된 풀을 순환(`i % pool.len`)해 모든 타깃이 댓글을 받고
///   풀의 댓글들이 고르게 재사용된다.
/// - **댓글 > 타깃**: 각 타깃이 셔플된 풀 앞쪽에서 서로 다른 댓글을 받는다
///   (남는 댓글은 쓰이지 않음).
///
/// `count == 0`이거나 풀이 비면 `[]`.
pub fn distribute_comments(
    count: usize,
    comments: &[String],
    rng: &mut impl FnMut() -> f64,
) -> Vec<String> {
    if count == 0 || comments.is_empty() {
        return Vec::new();
    }
    let pool = shuffle(comments, rng);
    (0..count).map(|i| pool[i % pool.len()].clone()).collect()
}

/// 댓글 풀을 셔플해 `buckets`개 계정에 **고르게 나눠 담는다**(#400 나눠서게시). `distribute_comments`가
/// 계정당 1개만 주는 것과 달리, 댓글이 계정보다 많으면 각 계정이 여러 개를 받는다(겹침 없이 전량 소진).
/// 앞 버킷부터 1개씩 더 받아 개수 차이를 최대 1로 만든다(프론트 `distributeStocksEvenly`와 동일 규칙).
///
/// 예: 4댓글·2계정 → 각 2개; 5댓글·2계정 → [3, 2]. 댓글이 계정보다 적으면 뒤 버킷은 빈 배열
/// (UI가 `댓글 수 ≥ 계정 수`를 강제하므로 통상 빈 버킷은 없다). `buckets == 0`이거나 풀이 비면 `[]`.
pub fn partition_comments(
    buckets: usize,
    comments: &[String],
    rng: &mut impl FnMut() -> f64,
) -> Vec<Vec<String>> {
    if buckets == 0 || comments.is_empty() {
        return Vec::new();
    }
    let pool = shuffle(comments, rng);
    let base = pool.len() / buckets;
    let remainder = pool.len() % buckets;
    let mut out: Vec<Vec<String>> = Vec::with_capacity(buckets);
    let mut cursor = 0;
    for b in 0..buckets {
        // 앞에서 remainder개 버킷만 base+1개를 받아 동등하게(차이 ≤ 1) 나눈다.
        let take = base + if b < remainder { 1 } else { 0 };
        out.push(pool[cursor..cursor + take].to_vec());
        cursor += take;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 정확한 Fisher–Yates 순열을 강제하는 결정적 rng 스텁(값을 순환 반환).
    /// 분배가 실제로 셔플을 거치는지(입력 순서 그대로가 아닌지) 증명한다.
    fn seq_rng(values: Vec<f64>) -> impl FnMut() -> f64 {
        let mut i = 0;
        move || {
            let v = values[i % values.len()];
            i += 1;
            v
        }
    }

    fn pool(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mulberry32_is_deterministic_for_a_fixed_seed() {
        let mut a = mulberry32(123);
        let mut b = mulberry32(123);
        let seq_a = [a(), a(), a(), a()];
        let seq_b = [b(), b(), b(), b()];
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn mulberry32_yields_values_in_unit_interval() {
        let mut rng = mulberry32(42);
        for _ in 0..100 {
            let v = rng();
            assert!((0.0..1.0).contains(&v));
        }
    }

    #[test]
    fn mulberry32_differs_across_seeds() {
        assert_ne!(mulberry32(1)(), mulberry32(2)());
    }

    #[test]
    fn distribute_assigns_one_comment_per_target() {
        let comments = pool(&["c1", "c2", "c3"]);
        let mut rng = mulberry32(7);
        let got = distribute_comments(3, &comments, &mut rng);
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|c| comments.contains(c)));
    }

    #[test]
    fn distribute_is_deterministic_for_a_fixed_seed() {
        let comments = pool(&["c1", "c2", "c3", "c4", "c5"]);
        let first = distribute_comments(5, &comments, &mut mulberry32(99));
        let second = distribute_comments(5, &comments, &mut mulberry32(99));
        assert_eq!(first, second);
    }

    #[test]
    fn distribute_routes_through_the_shuffle() {
        // rng=[0]은 2원소 Fisher–Yates를 뒤집는다: ["x","y"] → ["y","x"].
        // 따라서 첫 타깃은 "y"(셔플됨)여야 하고 "x"(입력 순서)가 아니다 —
        // 분배가 셔플을 건너뛰고 입력 순서대로 배정하면 실패한다.
        let comments = pool(&["x", "y"]);
        let got = distribute_comments(2, &comments, &mut seq_rng(vec![0.0]));
        assert_eq!(got, pool(&["y", "x"]));
    }

    #[test]
    fn distribute_boundary_fewer_comments_than_targets_reuses_pool() {
        let comments = pool(&["c1", "c2"]);
        let got = distribute_comments(5, &comments, &mut mulberry32(11));
        assert_eq!(got.len(), 5);
        assert!(got.iter().all(|c| comments.contains(c)));
        // 다섯 타깃에 걸쳐 두 댓글이 모두 쓰인다(풀 순환).
        let used: HashSet<&String> = got.iter().collect();
        assert_eq!(used.len(), 2);
    }

    #[test]
    fn distribute_boundary_more_comments_than_targets_are_distinct() {
        let comments = pool(&["c1", "c2", "c3", "c4", "c5"]);
        let got = distribute_comments(2, &comments, &mut mulberry32(5));
        assert_eq!(got.len(), 2);
        let used: HashSet<&String> = got.iter().collect();
        assert_eq!(used.len(), 2);
    }

    #[test]
    fn distribute_is_empty_without_targets_or_comments() {
        assert!(distribute_comments(0, &pool(&["c1"]), &mut mulberry32(1)).is_empty());
        assert!(distribute_comments(1, &[], &mut mulberry32(1)).is_empty());
    }

    #[test]
    fn distribute_single_comment_goes_to_every_target() {
        let comments = pool(&["only"]);
        let got = distribute_comments(3, &comments, &mut mulberry32(1));
        assert_eq!(got, pool(&["only", "only", "only"]));
    }

    #[test]
    fn partition_splits_comments_evenly_and_uses_all() {
        // #400: 4댓글·2계정 → 계정당 2개, 전량 소진, 겹침 없음.
        let comments = pool(&["a", "b", "c", "d"]);
        let got = partition_comments(2, &comments, &mut mulberry32(7));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].len(), 2);
        assert_eq!(got[1].len(), 2);
        let mut all: Vec<String> = got.into_iter().flatten().collect();
        all.sort();
        assert_eq!(all, pool(&["a", "b", "c", "d"]));
    }

    #[test]
    fn partition_gives_remainder_to_front_buckets() {
        // 5댓글·2계정 → [3, 2] (앞 버킷이 나머지를 받는다).
        let comments = pool(&["a", "b", "c", "d", "e"]);
        let got = partition_comments(2, &comments, &mut mulberry32(3));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].len(), 3);
        assert_eq!(got[1].len(), 2);
        let total: usize = got.iter().map(|b| b.len()).sum();
        assert_eq!(total, 5);
    }

    #[test]
    fn partition_is_empty_without_buckets_or_comments() {
        assert!(partition_comments(0, &pool(&["c1"]), &mut mulberry32(1)).is_empty());
        assert!(partition_comments(2, &[], &mut mulberry32(1)).is_empty());
    }

    #[test]
    fn partition_leaves_trailing_buckets_empty_when_fewer_comments() {
        // 계정 > 댓글 (UI가 막지만 안전): 앞 버킷만 채우고 뒤는 빈 배열.
        let comments = pool(&["a"]);
        let got = partition_comments(3, &comments, &mut mulberry32(1));
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], pool(&["a"]));
        assert!(got[1].is_empty());
        assert!(got[2].is_empty());
    }
}
