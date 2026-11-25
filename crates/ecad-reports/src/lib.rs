//! Reports are queries on the model, so they can never disagree with the drawings.

use ecad_model::{ModelError, Project};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireListRow {
    pub wire: String,
    pub from_tag: String,
    pub from_point: String,
    pub to_tag: String,
    pub to_point: String,
}

/// One row per connection, sorted by wire number.
pub fn wire_list(project: &Project) -> Result<Vec<WireListRow>, ModelError> {
    project
        .connections
        .values()
        .map(|c| {
            let (from_tag, from_point) = project.describe(&c.from)?;
            let (to_tag, to_point) = project.describe(&c.to)?;
            Ok(WireListRow { wire: c.wire_number.clone().unwrap_or_default(), from_tag, from_point, to_tag, to_point })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|mut rows| {
            rows.sort_by(|a, b| a.wire.cmp(&b.wire));
            rows
        })
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

pub fn wire_list_csv(rows: &[WireListRow]) -> String {
    let mut out = String::from("wire,from_device,from_point,to_device,to_point\n");
    for r in rows {
        let fields = [&r.wire, &r.from_tag, &r.from_point, &r.to_tag, &r.to_point];
        out += &fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ecad_model::sample::start_circuit;

    #[test]
    fn wire_list_of_the_sample() {
        let csv = wire_list_csv(&wire_list(&start_circuit().unwrap()).unwrap());
        assert_eq!(
            csv,
            "wire,from_device,from_point,to_device,to_point\n\
             W1,-X1:L+,1,-S1,13\n\
             W2,-S1,14,-K1,A1\n\
             W3,-K1,A2,-X1:M,1\n"
        );
    }

    #[test]
    fn csv_quotes_awkward_fields() {
        assert_eq!(csv_field("=A1+B2-K1"), "=A1+B2-K1");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
