use anyhow::Result;
use clap::Parser;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Debug, Parser)]
#[command(version, about = "Rust version of `cal`")]
pub struct Args {
    /// Year
    #[arg(value_name = "YEAR", value_parser = clap::value_parser!(i32).range(1..=9999))]
    year: Option<i32>,

    /// Month
    #[arg(
        short,
        value_name = "MONTH",
        value_parser = |month: &str| parse_month(month.to_string())
    )]
    month: Option<u32>,

    /// Display the current year
    #[arg(
        short = 'y',
        long = "year",
        conflicts_with_all = ["month", "year"]
    )]
    whole_year: bool,
}

fn run(args: Args) -> Result<()> {
    let mode = if args.whole_year {
        "current whole year"
    } else if let Some(month) = args.month {
        println!("specified month: {}", MONTHS[(month - 1) as usize]);
        return Ok(());
    } else if args.year.is_some() {
        "specified whole year"
    } else {
        "current month"
    };

    println!("{mode}");
    Ok(())
}

fn parse_month(month: String) -> Result<u32> {
    if let Ok(month_num) = month.parse::<u32>() {
        if (1..=12).contains(&month_num) {
            return Ok(month_num);
        }
        anyhow::bail!(r#"month "{month}" not in the range 1 through 12"#);
    }

    let month = month.to_lowercase();
    let matches: Vec<_> = MONTHS
        .iter()
        .enumerate()
        .filter(|(_, name)| name.to_lowercase().starts_with(&month))
        .collect();

    if matches.len() == 1 {
        Ok((matches[0].0 + 1) as u32)
    } else {
        anyhow::bail!(r#"Invalid month "{month}""#)
    }
}

fn main() {
    if let Err(e) = run(Args::parse()) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::parse_month;

    #[test]
    fn test_parse_month() {
        let res = parse_month("1".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 1u32);

        let res = parse_month("12".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 12u32);

        let res = parse_month("jan".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), 1u32);

        let res = parse_month("0".to_string());
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            r#"month "0" not in the range 1 through 12"#
        );

        let res = parse_month("13".to_string());
        assert!(res.is_err());
        assert_eq!(
            res.unwrap_err().to_string(),
            r#"month "13" not in the range 1 through 12"#
        );

        let res = parse_month("foo".to_string());
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), r#"Invalid month "foo""#);
    }

//     #[test]
//     fn test_format_month() {
//         let today = NaiveDate::from_ymd_opt(0, 1, 1).unwrap();
//         let leap_february = vec![
//             "   February 2020      ",
//             "Su Mo Tu We Th Fr Sa  ",
//             "                   1  ",
//             " 2  3  4  5  6  7  8  ",
//             " 9 10 11 12 13 14 15  ",
//             "16 17 18 19 20 21 22  ",
//             "23 24 25 26 27 28 29  ",
//             "                      ",
//         ];
//         assert_eq!(format_month(2020, 2, true, today), leap_february);

//         let may = vec![
//             "        May           ",
//             "Su Mo Tu We Th Fr Sa  ",
//             "                1  2  ",
//             " 3  4  5  6  7  8  9  ",
//             "10 11 12 13 14 15 16  ",
//             "17 18 19 20 21 22 23  ",
//             "24 25 26 27 28 29 30  ",
//             "31                    ",
//         ];
//         assert_eq!(format_month(2020, 5, false, today), may);

//         let april_hl = vec![
//             "     April 2021       ",
//             "Su Mo Tu We Th Fr Sa  ",
//             "             1  2  3  ",
//             " 4  5  6 \u{1b}[7m 7\u{1b}[0m  8  9 10  ",
//             "11 12 13 14 15 16 17  ",
//             "18 19 20 21 22 23 24  ",
//             "25 26 27 28 29 30     ",
//             "                      ",
//         ];
//         let today = NaiveDate::from_ymd_opt(2021, 4, 7).unwrap();
//         assert_eq!(format_month(2021, 4, true, today), april_hl);
//     }

//     #[test]
//     fn test_last_day_in_month() {
//         assert_eq!(
//             last_day_in_month(2020, 1),
//             NaiveDate::from_ymd_opt(2020, 1, 31).unwrap()
//         );
//         assert_eq!(
//             last_day_in_month(2020, 2),
//             NaiveDate::from_ymd_opt(2020, 2, 29).unwrap()
//         );
//         assert_eq!(
//             last_day_in_month(2020, 4),
//             NaiveDate::from_ymd_opt(2020, 4, 30).unwrap()
//         );
//     }
}
