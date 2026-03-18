use super::*;
use super::helpers::*;

pub(super) fn render_event_log(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.theme;
    let total = app.event_log.len();
    let title: String = match app.log_category_filter {
        None => "◆ SYSTEM LOG".to_string(),
        Some(cat) => {
            let filtered = app.event_log.iter().filter(|e| e.category == cat).count();
            format!("◆ EVENT LOG [{}/{}] [{}]", filtered, total, cat.label())
        }
    };
    let block = primary_block(&title, t);

    if app.event_log.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "  No events recorded",
            Style::default().fg(t.muted),
        )))
        .block(block);
        f.render_widget(p, area);
        return;
    }

    let inner = block.inner(area);
    let visible = inner.height as usize;

    let filtered_entries: Vec<&LogEntry> = match app.log_category_filter {
        None => app.event_log.iter().collect(),
        Some(cat) => app.event_log.iter().filter(|e| e.category == cat).collect(),
    };

    let items: Vec<ListItem> = filtered_entries
        .iter()
        .skip(app.log_scroll)
        .take(visible)
        .map(|entry| {
            let cat_color = log_category_color(entry.category, t);

            ListItem::new(Line::from(vec![
                Span::styled(format!(" {} ", entry.timestamp), Style::default().fg(t.muted)),
                Span::styled(
                    format!("[{}]", entry.category.label()),
                    Style::default()
                        .fg(cat_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {}", entry.message), Style::default().fg(t.bright)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}
