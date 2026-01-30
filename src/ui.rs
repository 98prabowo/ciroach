use std::collections::{HashMap, HashSet};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

pub struct PreFlightUI {
    _multi: MultiProgress,
    bars: HashMap<String, ProgressBar>,
}

impl PreFlightUI {
    pub fn new(images: &HashSet<String>) -> Self {
        let multi = MultiProgress::new();
        let mut bars = HashMap::new();

        let style =
            ProgressStyle::with_template("  {elapsed_precise} {bar:30.white/black} WAIT {msg}")
                .unwrap()
                .progress_chars("·  ");

        for img in images {
            let pb = multi.add(ProgressBar::new(0));
            pb.set_style(style.clone());
            pb.set_message(format!("PULL {}", img));
            bars.insert(img.clone(), pb);
        }

        Self {
            _multi: multi,
            bars,
        }
    }

    pub fn update_progress(&self, img: &str, current: u64, total: u64) {
        if let Some(pb) = self.bars.get(img) {
            pb.set_length(total);
            pb.set_position(current);
            pb.set_style(
                ProgressStyle::with_template(
                    "  {elapsed_precise} {bar:30.cyan/blue} {bytes}/{total_bytes} {msg}",
                )
                .unwrap()
                .progress_chars("#> "),
            );
            pb.set_message(format!("PULLING {}", img));
        }
    }

    pub fn succeed_image(&self, img: &str) {
        if let Some(pb) = self.bars.get(img) {
            pb.set_length(100);
            pb.set_position(100);
            pb.set_style(
                ProgressStyle::with_template("  {elapsed_precise} {bar:30.green/green} DONE {msg}")
                    .unwrap()
                    .progress_chars("##"),
            );
            pb.finish_with_message(img.to_string());
        }
    }

    pub fn failed_image(&self, img: &str) {
        if let Some(pb) = self.bars.get(img) {
            pb.set_style(
                ProgressStyle::with_template("  {elapsed_precise} {bar:30.red/red} ERROR {msg}")
                    .unwrap(),
            );
            pb.abandon_with_message(format!("❌ Failed {}", img));
        }
    }
}
