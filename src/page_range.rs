use crate::error::Error;

/// Parse a page range string like "1-10", "3,5,7", "1-5,8,10-12" into a sorted Vec of 1-based page numbers.
fn parse_page_number(s: &str) -> Result<u32, Error> {
    s.trim()
        .parse()
        .map_err(|_| Error::InvalidArgs(format!("invalid page number: {s}")))
}

fn validate_page(page: u32, max_page: u32) -> Result<(), Error> {
    if page == 0 {
        return Err(Error::InvalidArgs("page numbers are 1-based".into()));
    }
    if page > max_page {
        return Err(Error::InvalidArgs(format!(
            "page {page} exceeds page count {max_page}"
        )));
    }
    Ok(())
}

pub fn parse_page_range(input: &str, max_page: u32) -> Result<Vec<u32>, Error> {
    let mut pages = Vec::new();

    for part in input.split(',') {
        let part = part.trim();
        if let Some((start_str, end_str)) = part.split_once('-') {
            let start = parse_page_number(start_str)?;
            let end = parse_page_number(end_str)?;
            validate_page(start, max_page)?;
            validate_page(end, max_page)?;
            if start > end {
                return Err(Error::InvalidArgs(format!(
                    "invalid range: {start} > {end}"
                )));
            }
            pages.extend(start..=end);
        } else {
            let page = parse_page_number(part)?;
            validate_page(page, max_page)?;
            pages.push(page);
        }
    }

    pages.sort_unstable();
    pages.dedup();
    Ok(pages)
}

/// Divide pages into roughly equal chunks for worker distribution.
pub fn divide_pages(total_pages: u32, num_workers: u32) -> Vec<(u32, u32)> {
    if total_pages == 0 || num_workers == 0 {
        return Vec::new();
    }

    let workers = num_workers.min(total_pages);
    let base_size = total_pages / workers;
    let remainder = total_pages % workers;

    let mut ranges = Vec::with_capacity(workers as usize);
    let mut start = 1;

    for i in 0..workers {
        let chunk = base_size + if i < remainder { 1 } else { 0 };
        let end = start + chunk - 1;
        ranges.push((start, end));
        start = end + 1;
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_page() {
        assert_eq!(parse_page_range("5", 10).unwrap(), vec![5]);
    }

    #[test]
    fn parse_range() {
        assert_eq!(parse_page_range("1-5", 10).unwrap(), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn parse_mixed() {
        assert_eq!(
            parse_page_range("1-3,7,9-10", 10).unwrap(),
            vec![1, 2, 3, 7, 9, 10]
        );
    }

    #[test]
    fn parse_dedup() {
        assert_eq!(
            parse_page_range("1-5,3-7", 10).unwrap(),
            vec![1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn parse_exceeds_max() {
        assert!(parse_page_range("1-11", 10).is_err());
    }

    #[test]
    fn parse_zero_page() {
        assert!(parse_page_range("0", 10).is_err());
    }

    #[test]
    fn divide_evenly() {
        assert_eq!(divide_pages(12, 4), vec![(1, 3), (4, 6), (7, 9), (10, 12)]);
    }

    #[test]
    fn divide_with_remainder() {
        assert_eq!(divide_pages(10, 3), vec![(1, 4), (5, 7), (8, 10)]);
    }

    #[test]
    fn divide_more_workers_than_pages() {
        assert_eq!(divide_pages(2, 5), vec![(1, 1), (2, 2)]);
    }
}
