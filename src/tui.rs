use crate::{model::Session,scan};
use anyhow::Result;
use crossterm::{event::{self,Event,KeyCode,KeyEventKind},execute,terminal::{disable_raw_mode,enable_raw_mode,EnterAlternateScreen,LeaveAlternateScreen}};
use ratatui::{backend::CrosstermBackend,layout::{Constraint,Direction,Layout},style::{Color,Modifier,Style},widgets::{Block,Borders,List,ListItem,Paragraph,Wrap},Terminal};
use std::{io, time::Duration};

struct RawGuard;
impl RawGuard {
    fn enter()->Result<Self>{enable_raw_mode()?;execute!(io::stdout(),EnterAlternateScreen)?;Ok(Self)}
}
impl Drop for RawGuard {fn drop(&mut self){let _=disable_raw_mode();let _=execute!(io::stdout(),LeaveAlternateScreen);}}

fn show_name(s:&Session)->String {
    let short=s.id.chars().take(24).collect::<String>();
    format!("{} | {} | {} | {}",s.source,short,s.model,s.phase)
}
pub fn monitor()->Result<()> {
    let _guard=RawGuard::enter()?;
    let backend=CrosstermBackend::new(io::stdout());
    let mut term=Terminal::new(backend)?;
    let mut sessions=scan::scan_checked()?;
    let mut selected=0usize;
    loop {
        term.draw(|f|{
            let base=f.area();
            let columns=Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3),Constraint::Min(3),Constraint::Length(1)]).split(base);
            let title=Paragraph::new("TREILLS  |  Claude Code + Codex  |  local / SSH").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)).block(Block::default().borders(Borders::ALL));
            f.render_widget(title,columns[0]);
            let body=Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(38),Constraint::Percentage(62)]).split(columns[1]);
            let rows:Vec<ListItem>=sessions.iter().enumerate().map(|(i,s)|{
                let style=if i==selected {Style::default().bg(Color::DarkGray).fg(Color::Yellow)}else{Style::default()};
                ListItem::new(show_name(s)).style(style)
            }).collect();
            f.render_widget(List::new(rows).block(Block::default().title(format!("Sessions ({})",sessions.len())).borders(Borders::ALL)),body[0]);
            if let Some(s)=sessions.get(selected){
                let mut details=vec![
                    format!("Source: {}   Session: {}",s.source,s.id),
                    format!("Project: {}",s.cwd),
                    format!("Model: {}   Phase: {}",s.model,s.phase),
                    format!("Last file activity: {} (unix s)",s.last_epoch),
                    String::new(),"Tools".into()
                ];
                details.extend(s.tools.iter().map(|(k,v)|format!("  {k}: {v}")));
                details.push(String::new());details.push("Skills".into());
                details.extend(s.skills.iter().map(|(k,v)|format!("  {k}: {v}")));
                details.push(String::new());details.push("Active agents".into());
                details.extend(s.agents.iter().map(|(k,v)|format!("  {v} ({k})")));
                details.push(String::new());details.push("Recent activity".into());
                details.extend(s.activity.iter().rev().take(16).rev().cloned());
                f.render_widget(Paragraph::new(details.join("\n")).wrap(Wrap{trim:false}).block(Block::default().title("Details / activity").borders(Borders::ALL)),body[1]);
            }else{
                f.render_widget(Paragraph::new("No sessions found. Run Claude/Codex here or install metadata hooks.").block(Block::default().borders(Borders::ALL)),body[1]);
            }
            f.render_widget(Paragraph::new("Up/Down select  |  r refresh  |  q quit  |  refresh every 2s"),columns[2]);
        })?;
        if event::poll(Duration::from_secs(2))? {
            if let Event::Key(key)=event::read()? {
                if key.kind==KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q')|KeyCode::Esc=>break,
                        KeyCode::Down=>{if selected+1<sessions.len(){selected+=1;}},
                        KeyCode::Up=>{selected=selected.saturating_sub(1);},
                        KeyCode::Char('r')=>{},
                        _=>{}
                    }
                }
            }
        }
        let selected_id=sessions.get(selected).map(|s|format!("{}:{}",s.source,s.id));
        match scan::scan_checked() {
            Ok(next)=>{
                if let Some(id)=selected_id {
                    selected=next.iter().position(|s|format!("{}:{}",s.source,s.id)==id)
                        .unwrap_or_else(||selected.min(next.len().saturating_sub(1)));
                }else{selected=selected.min(next.len().saturating_sub(1));}
                sessions=next;
            }
            Err(err)=>eprintln!("treills: refresh failed (keeping last snapshot): {err:#}"),
        }
    }
    Ok(())
}
