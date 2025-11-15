pub mod nalgebra;
pub mod palette;
pub mod std;

use crate::util::Boo;

pub trait PropertiesUi {
    type Config: Default;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response;
}

#[derive(Clone, Debug, Default)]
pub struct TrackChanges {
    pub changed: bool,
}

impl TrackChanges {
    pub fn track<R>(&mut self, response: R) -> R
    where
        R: HasChangeValue,
    {
        self.changed |= response.changed();
        response
    }

    pub fn propagate<R>(self, response: &mut R)
    where
        R: HasChangeValue,
    {
        if self.changed {
            response.mark_changed();
        }
    }

    pub fn propagated<R>(self, mut response: R) -> R
    where
        R: HasChangeValue,
    {
        self.propagate(&mut response);
        response
    }

    pub fn propagated_and<R>(self, mut response: R, if_changed: impl FnOnce()) -> R
    where
        R: HasChangeValue,
    {
        if self.changed {
            response.mark_changed();
            if_changed();
        }
        response
    }
}

pub trait HasChangeValue {
    fn changed(&self) -> bool;
    fn mark_changed(&mut self);
}

impl HasChangeValue for egui::Response {
    fn changed(&self) -> bool {
        egui::Response::changed(self)
    }

    fn mark_changed(&mut self) {
        egui::Response::mark_changed(self);
    }
}

impl<R> HasChangeValue for egui::InnerResponse<R> {
    fn changed(&self) -> bool {
        self.response.changed()
    }

    fn mark_changed(&mut self) {
        self.response.mark_changed();
    }
}

impl HasChangeValue for TrackChanges {
    fn changed(&self) -> bool {
        self.changed
    }

    fn mark_changed(&mut self) {
        self.changed = true;
    }
}

#[derive(Debug)]
pub struct Deletable<'a, T> {
    pub inner: &'a mut T,
    pub deletion_requested: bool,
}

impl<'a, T> Deletable<'a, T> {
    pub fn new(inner: &'a mut T) -> Self {
        Self {
            inner,
            deletion_requested: false,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub struct DeletableConfig<'a, C> {
    pub inner: Boo<'a, C>,
    // todo: config for delete button
}

impl<'a, T> PropertiesUi for Deletable<'a, T>
where
    T: PropertiesUi,
{
    type Config = DeletableConfig<'a, T::Config>;

    fn properties_ui(&mut self, ui: &mut egui::Ui, config: &Self::Config) -> egui::Response {
        let mut response = self.inner.properties_ui(ui, &config.inner);

        if ui.small_button("Delete").clicked() {
            self.deletion_requested = true;
            response.mark_changed();
        }

        response
    }
}

#[derive(Debug)]
pub struct PropertiesWidget<'a, P>
where
    P: PropertiesUi,
{
    pub config: Boo<'a, P::Config>,
    pub value: &'a mut P,
}

impl<'a, P> PropertiesWidget<'a, P>
where
    P: PropertiesUi,
{
    pub fn new(value: &'a mut P) -> Self {
        Self {
            config: Default::default(),
            value,
        }
    }

    pub fn with_config(mut self, config: impl Into<Boo<'a, P::Config>>) -> Self {
        self.config = config.into();
        self
    }
}

impl<'a, P> egui::Widget for PropertiesWidget<'a, P>
where
    P: PropertiesUi,
{
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        self.value.properties_ui(ui, &self.config)
    }
}

pub trait PropertiesUiExt {
    fn properties<P>(&mut self, properties: &mut P) -> egui::Response
    where
        P: PropertiesUi;
}

impl PropertiesUiExt for egui::Ui {
    fn properties<P>(&mut self, properties: &mut P) -> egui::Response
    where
        P: PropertiesUi,
    {
        properties.properties_ui(self, &Default::default())
    }
}

pub fn label_and_value<P>(
    ui: &mut egui::Ui,
    label: &str,
    changes: &mut TrackChanges,
    field: &mut P,
) -> egui::Response
where
    P: PropertiesUi,
{
    ui.horizontal(|ui| {
        ui.label(label);
        changes.track(field.properties_ui(ui, &Default::default()))
    })
    .inner
}
