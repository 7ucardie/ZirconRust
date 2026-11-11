//! Bounded, deterministic discovery of fully validated authored-piece joins.
use crate::{piece_ports::Side, wall_piece::Piece};
use std::path::Path;

pub const MAX_SOURCES: usize = 16;
pub const MAX_ATTEMPTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub source_index: usize,
    pub current_port: usize,
    pub candidate_port: usize,
    pub size: (u16, u16),
    pub remaining_ports: usize,
}

/// Results follow source, current connection, then candidate connection order.
/// The cheap filter only reduces work: every result passes `Piece::join`,
/// including coverage, artwork, and all unused-connection checks. No composed
/// maps are retained. The caller must revalidate the chosen join when applying.
/// Oversized searches fail before joining rather than reporting partial results.
pub fn enumerate(
    current: &Piece,
    candidates: &[Piece],
    assets: &Path,
) -> anyhow::Result<Vec<Candidate>> {
    let mut budget = MAX_ATTEMPTS;
    enumerate_with_budget(current, candidates, assets, &mut budget)
}

/// Share a join-trial budget across multiple discovery rounds.
/// A rejected preflight leaves the budget unchanged and returns no partial list.
pub fn enumerate_with_budget(
    current: &Piece,
    candidates: &[Piece],
    assets: &Path,
    remaining: &mut usize,
) -> anyhow::Result<Vec<Candidate>> {
    anyhow::ensure!(
        candidates.len() <= MAX_SOURCES,
        "Search at most {MAX_SOURCES} source pieces at once."
    );
    anyhow::ensure!(
        current.ports.len() <= 16 && candidates.iter().all(|p| p.ports.len() <= 16),
        "A piece supports at most 16 connections."
    );
    let mut eligible = vec![];
    for (source_index, piece) in candidates.iter().enumerate() {
        for (current_port, a) in current.ports.iter().enumerate() {
            for (candidate_port, b) in piece.ports.iter().enumerate() {
                let opposite = matches!(
                    (a.side, b.side),
                    (Side::North, Side::South)
                        | (Side::South, Side::North)
                        | (Side::East, Side::West)
                        | (Side::West, Side::East)
                );
                if opposite && a.width == b.width && a.profile == b.profile {
                    anyhow::ensure!(
                        eligible.len() < MAX_ATTEMPTS.min(*remaining),
                        "Connection search exceeds the remaining join budget ({}). Search fewer pieces or use more specific profiles.", (*remaining).min(MAX_ATTEMPTS)
                    );
                    eligible.push((source_index, current_port, candidate_port));
                }
            }
        }
    }
    *remaining -= eligible.len();
    let mut found = vec![];
    for (source_index, current_port, candidate_port) in eligible {
        if let Ok(joined) = current.join(
            &candidates[source_index],
            current_port,
            candidate_port,
            assets,
        ) {
            found.push(Candidate {
                source_index,
                current_port,
                candidate_port,
                size: joined.size(),
                remaining_ports: joined.ports.len(),
            });
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{piece_ports::Port, tile_doc::Stamp, wall_piece::Kind};
    use mir_formats::map::MapFile;

    fn port(side: Side, offset: u16) -> Port {
        Port {
            side,
            offset,
            width: 1,
            ..Port::default()
        }
    }
    fn piece(width: u16, height: u16, ports: Vec<Port>) -> Piece {
        let mut map = MapFile::blank(width, height);
        for x in 0..i32::from(width) {
            for y in 0..i32::from(height) {
                let cell = map.cell_mut(x, y).unwrap();
                cell.back_file = 255;
                cell.flag = 3;
            }
        }
        let mut piece = Piece::capture(
            &Stamp::capture(&map, (0, 0), (i32::from(width) - 1, i32::from(height) - 1)),
            "Fixture".into(),
            Kind::Corner,
            Path::new("."),
        )
        .unwrap();
        piece.ports = ports;
        piece
    }

    #[test]
    fn shared_budget_counts_rejected_trials_and_fails_before_partial_search() {
        let current = piece(4, 4, vec![port(Side::East, 0)]);
        let sources = vec![
            piece(4, 4, vec![port(Side::West, 0)]),
            piece(4, 4, vec![port(Side::West, 1)]),
        ];
        let mut budget = 1;
        assert!(enumerate_with_budget(&current, &sources, Path::new("."), &mut budget).is_err());
        assert_eq!(budget, 1);
        budget = 3;
        assert_eq!(
            enumerate_with_budget(&current, &sources, Path::new("."), &mut budget)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(budget, 1); // The parity-invalid trial still consumes work.
        enumerate_with_budget(&current, &sources[..1], Path::new("."), &mut budget).unwrap();
        assert_eq!(budget, 0);
        assert!(
            enumerate_with_budget(&current, &sources[..1], Path::new("."), &mut budget).is_err()
        );
    }

    #[test]
    fn discovery_is_stable_and_reports_only_fully_valid_joins() {
        let current = piece(4, 4, vec![port(Side::North, 2), port(Side::East, 0)]);
        let sources = vec![
            piece(4, 4, vec![port(Side::West, 0), port(Side::East, 0)]),
            piece(4, 4, vec![port(Side::West, 1)]), // Back-tile parity fails.
            piece(4, 4, vec![port(Side::South, 2)]),
            piece(64, 4, vec![port(Side::West, 0)]), // Combined size fails.
        ];
        let found = enumerate(&current, &sources, Path::new(".")).unwrap();
        assert_eq!(
            found,
            vec![
                Candidate {
                    source_index: 0,
                    current_port: 1,
                    candidate_port: 0,
                    size: (8, 4),
                    remaining_ports: 2,
                },
                Candidate {
                    source_index: 2,
                    current_port: 0,
                    candidate_port: 0,
                    size: (4, 8),
                    remaining_ports: 1,
                },
            ]
        );
        assert_eq!(
            enumerate(&current, &sources, Path::new(".")).unwrap(),
            found
        );
        assert_eq!(current.size(), (4, 4));
        assert_eq!(current.ports.len(), 2);
        assert!(enumerate(&current, &[], Path::new(".")).unwrap().is_empty());
    }

    #[test]
    fn coverage_overlap_and_obstructed_unused_ports_are_not_suggestions() {
        let current = piece(4, 4, vec![port(Side::North, 2), port(Side::East, 0)]);
        let other = piece(4, 4, vec![port(Side::West, 2)]);
        let recessed = current.join(&other, 1, 0, Path::new(".")).unwrap();
        let sources = vec![
            piece(4, 2, vec![port(Side::South, 2)]),
            piece(8, 4, vec![port(Side::South, 2)]),
        ];
        let found = enumerate(&recessed, &sources, Path::new(".")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source_index, 0);
        assert_eq!(found[0].size, (8, 6));
        assert_eq!(found[0].remaining_ports, 0);

        let exposed = current
            .join(
                &piece(4, 4, vec![port(Side::West, 2), port(Side::West, 0)]),
                1,
                0,
                Path::new("."),
            )
            .unwrap();
        assert!(enumerate(&exposed, &sources[..1], Path::new("."))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn excessive_search_is_rejected_instead_of_truncated() {
        let current = piece(4, 4, vec![port(Side::East, 0)]);
        let too_many: Vec<_> = (0..=MAX_SOURCES).map(|_| piece(4, 4, vec![])).collect();
        assert!(enumerate(&current, &too_many, Path::new(".")).is_err());
        let invalid = piece(4, 4, vec![port(Side::West, 0); 17]);
        assert!(enumerate(&current, &[invalid], Path::new(".")).is_err());

        // Disjoint valid openings create 512 potentially matching pairs.
        let current = piece(4, 32, (0..16).map(|n| port(Side::East, n * 2)).collect());
        let many = (0..2)
            .map(|_| piece(4, 32, (0..16).map(|n| port(Side::West, n * 2)).collect()))
            .collect::<Vec<_>>();
        let error = enumerate(&current, &many, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("join budget (256)"));
    }
}
