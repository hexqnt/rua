//! Генерация файлов, помогающих поисковым роботам индексировать отчёт.

use std::fs;
use std::io;
use std::path::Path;

use chrono::NaiveDate;

use super::SITE_URL;

const SITEMAP_FILE_NAME: &str = "sitemap.xml";
const ROBOTS_FILE_NAME: &str = "robots.txt";

pub(super) fn write_site_files(output_html: &Path, last_modified: NaiveDate) -> io::Result<()> {
    fs::write(
        output_html.with_file_name(SITEMAP_FILE_NAME),
        render_sitemap(last_modified),
    )?;
    fs::write(
        output_html.with_file_name(ROBOTS_FILE_NAME),
        render_robots(),
    )
}

fn render_sitemap(last_modified: NaiveDate) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
           <url>\n\
             <loc>{SITE_URL}</loc>\n\
             <lastmod>{last_modified}</lastmod>\n\
             <changefreq>daily</changefreq>\n\
             <priority>1.0</priority>\n\
           </url>\n\
         </urlset>\n"
    )
}

fn render_robots() -> String {
    format!("User-agent: *\nAllow: /\n\nSitemap: {SITE_URL}{SITEMAP_FILE_NAME}\n")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::NaiveDate;

    use super::{render_robots, render_sitemap};

    #[test]
    fn sitemap_describes_canonical_page() {
        let date = NaiveDate::from_ymd_opt(2026, 9, 18).expect("valid date");
        let sitemap = render_sitemap(date);

        assert!(sitemap.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(sitemap.contains("<loc>https://rua.hexq.ru/</loc>"));
        assert!(sitemap.contains("<lastmod>2026-09-18</lastmod>"));
        assert!(sitemap.contains("<changefreq>daily</changefreq>"));
    }

    #[test]
    fn robots_allows_crawling_and_points_to_sitemap() {
        let robots = render_robots();

        assert!(robots.contains("User-agent: *\nAllow: /"));
        assert!(robots.contains("Sitemap: https://rua.hexq.ru/sitemap.xml"));
    }

    #[test]
    fn seo_files_are_siblings_of_html() {
        let output = Path::new("dist/custom.html");

        assert_eq!(
            output.with_file_name("sitemap.xml"),
            Path::new("dist/sitemap.xml")
        );
        assert_eq!(
            output.with_file_name("robots.txt"),
            Path::new("dist/robots.txt")
        );
    }
}
