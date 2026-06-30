//! 계정 → 하위 균등+랜덤 분배(겹침 없음, ±1). 설계 §10-3.
//! 프론트 `distributeStocksEvenly` + 셔플과 동일 결과(서버가 분배 시 중복 금지 보장).

use rand::seq::SliceRandom;
use uuid::Uuid;

/// total을 buckets개로 균등 분할(차이 최대 1) + 나머지를 랜덤 버킷에 배분.
pub fn split_counts(total: usize, buckets: usize) -> Vec<usize> {
    if buckets == 0 {
        return vec![];
    }
    let base = total / buckets;
    let rem = total % buckets;
    let mut idx: Vec<usize> = (0..buckets).collect();
    idx.shuffle(&mut rand::thread_rng()); // 어느 대가 +1 받을지 무작위(§10-3)
    let plus: std::collections::HashSet<usize> = idx.into_iter().take(rem).collect();
    (0..buckets)
        .map(|i| base + usize::from(plus.contains(&i)))
        .collect()
}

/// 계정·기기를 셔플 후 균등 배분 → 기기별 계정 묶음. 한 계정 = 정확히 한 대(겹침 없음).
pub fn distribute(
    mut account_ids: Vec<Uuid>,
    mut device_ids: Vec<Uuid>,
) -> Vec<(Uuid, Vec<Uuid>)> {
    if device_ids.is_empty() || account_ids.is_empty() {
        return vec![];
    }
    let mut rng = rand::thread_rng();
    account_ids.shuffle(&mut rng);
    device_ids.shuffle(&mut rng);
    let counts = split_counts(account_ids.len(), device_ids.len());
    let mut out = Vec::with_capacity(device_ids.len());
    let mut cursor = 0usize;
    for (dev, n) in device_ids.into_iter().zip(counts) {
        let slice = account_ids[cursor..cursor + n].to_vec();
        cursor += n;
        out.push((dev, slice));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_counts_even_and_balanced() {
        // 18개 / 4대 → 합 18, 각 4~5(차이 1) — publish-helpers.test.ts와 동일 성질.
        let c = split_counts(18, 4);
        assert_eq!(c.iter().sum::<usize>(), 18);
        assert!(c.iter().all(|&x| x == 4 || x == 5));
        // 10 / 3 → 합 10, 4·3·3.
        let c = split_counts(10, 3);
        assert_eq!(c.iter().sum::<usize>(), 10);
        let mut s = c.clone();
        s.sort();
        assert_eq!(s, vec![3, 3, 4]);
    }

    #[test]
    fn distribute_is_a_partition_no_overlap() {
        let accts: Vec<Uuid> = (0..10).map(|_| Uuid::new_v4()).collect();
        let devs: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let result = distribute(accts.clone(), devs);
        // 모든 계정이 정확히 한 번씩 배정(겹침·누락 없음).
        let mut all: Vec<Uuid> = result.iter().flat_map(|(_, v)| v.clone()).collect();
        all.sort();
        let mut expect = accts.clone();
        expect.sort();
        assert_eq!(all, expect);
        assert_eq!(result.iter().map(|(_, v)| v.len()).sum::<usize>(), 10);
    }

    #[test]
    fn distribute_empty_inputs() {
        assert!(distribute(vec![], vec![Uuid::new_v4()]).is_empty());
        assert!(distribute(vec![Uuid::new_v4()], vec![]).is_empty());
    }
}
