use crate::app::App;
use crate::common::Tab;
use crate::game::{DrawBaselayer, State, Transition};
use crate::sandbox::dashboards::DashTab;
use crate::sandbox::SandboxMode;
use ezgui::{
    Autocomplete, Btn, Composite, EventCtx, GfxCtx, Line, LinePlot, Outcome, PlotOptions, Series,
    Widget,
};
use map_model::BusRouteID;

pub struct ActiveTraffic {
    composite: Composite,
}

impl ActiveTraffic {
    pub fn new(ctx: &mut EventCtx, app: &App) -> Box<dyn State> {
        let mut active_agents = vec![Series {
            label: format!("After \"{}\"", app.primary.map.get_edits().edits_name),
            color: app.cs.after_changes,
            pts: app
                .primary
                .sim
                .get_analytics()
                .active_agents(app.primary.sim.time()),
        }];
        if app.has_prebaked().is_some() {
            active_agents.push(Series {
                label: format!("Before \"{}\"", app.primary.map.get_edits().edits_name),
                color: app.cs.before_changes.alpha(0.5),
                pts: app
                    .prebaked()
                    .active_agents(app.primary.sim.get_end_of_day()),
            });
        }

        Box::new(ActiveTraffic {
            composite: Composite::new(Widget::col(vec![
                DashTab::ActiveTraffic.picker(ctx, app),
                LinePlot::new(ctx, active_agents, PlotOptions::fixed()),
            ]))
            .exact_size_percent(90, 90)
            .build(ctx),
        })
    }
}

impl State for ActiveTraffic {
    fn event(&mut self, ctx: &mut EventCtx, app: &mut App) -> Transition {
        match self.composite.event(ctx) {
            Some(Outcome::Clicked(x)) => DashTab::TripSummaries.transition(ctx, app, &x),
            None => Transition::Keep,
        }
    }

    fn draw_baselayer(&self) -> DrawBaselayer {
        DrawBaselayer::Custom
    }

    fn draw(&self, g: &mut GfxCtx, app: &App) {
        g.clear(app.cs.grass);
        self.composite.draw(g);
    }
}

pub struct TransitRoutes {
    composite: Composite,
}

impl TransitRoutes {
    pub fn new(ctx: &mut EventCtx, app: &App) -> Box<dyn State> {
        let mut routes = Vec::new();
        for r in app.primary.map.all_bus_routes() {
            routes.push((r.full_name.clone(), r.id));
        }
        // TODO Sort first by length, then lexicographically
        routes.sort();

        let col = vec![
            DashTab::TransitRoutes.picker(ctx, app),
            Line("Transit routes").small_heading().draw(ctx),
            Widget::row(vec![
                Widget::draw_svg(ctx, "system/assets/tools/search.svg"),
                Autocomplete::new(ctx, routes.iter().map(|(r, id)| (r.clone(), *id)).collect())
                    .named("search"),
            ])
            .padding(8),
            Widget::row(
                routes
                    .into_iter()
                    .map(|(r, id)| {
                        Btn::text_fg(r)
                            .build(ctx, id.to_string(), None)
                            .margin_below(10)
                    })
                    .collect(),
            )
            .flex_wrap(ctx, 80),
        ];

        Box::new(TransitRoutes {
            composite: Composite::new(Widget::col(col))
                .exact_size_percent(90, 90)
                .build(ctx),
        })
    }
}

impl State for TransitRoutes {
    fn event(&mut self, ctx: &mut EventCtx, app: &mut App) -> Transition {
        let route = match self.composite.event(ctx) {
            Some(Outcome::Clicked(x)) => {
                if let Some(x) = x.strip_prefix("BusRoute #") {
                    BusRouteID(x.parse::<usize>().unwrap())
                } else {
                    return DashTab::TransitRoutes.transition(ctx, app, &x);
                }
            }
            None => {
                if let Some(routes) = self.composite.autocomplete_done("search") {
                    if !routes.is_empty() {
                        routes[0]
                    } else {
                        return Transition::Keep;
                    }
                } else {
                    return Transition::Keep;
                }
            }
        };

        Transition::PopWithData(Box::new(move |state, ctx, app| {
            let sandbox = state.downcast_mut::<SandboxMode>().unwrap();
            let mut actions = sandbox.contextual_actions();
            sandbox.controls.common.as_mut().unwrap().launch_info_panel(
                ctx,
                app,
                Tab::BusRoute(route),
                &mut actions,
            )
        }))
    }

    fn draw_baselayer(&self) -> DrawBaselayer {
        DrawBaselayer::Custom
    }

    fn draw(&self, g: &mut GfxCtx, app: &App) {
        g.clear(app.cs.grass);
        self.composite.draw(g);
    }
}
