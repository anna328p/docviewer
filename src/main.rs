use std::{collections::HashSet, convert::identity};

use adw::{gdk::Display, prelude::*};
use glib::VariantDict;
use relm4::{MessageBroker, prelude::*, typed_view::list::*};
use serde::Deserialize;
use webkit6::{prelude::*, LoadEvent, WebView};

use gtk::Orientation;

type Nothing = ();

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManPageID {
    name: String,
    sections: HashSet<String>,
}

#[derive(Deserialize, Debug, Clone, Hash)]
struct HTMLHeading {
    tag_name: String,
    inner_text: String,
    index: usize,
    id: Option<String>,
}

impl HTMLHeading {
    fn indent_levels(&self) -> usize {
        match self.tag_name.to_lowercase().as_str() {
            "h1" => 0,
            "h2" => 1,
            "h3" => 2,
            "h4" => 3,
            "h5" => 4,
            "h6" => 5,
            _ => 0,
        }
    }
}

type Outline = Vec<HTMLHeading>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct NavigationState {
    can_go_back: bool,
    can_go_forward: bool,
}

#[derive(Debug)]
enum WebPaneMsg {
    GoBack,
    GoForward,
    UpdateNavState,
    UpdatedURI(String),
    SelectedHeading(HTMLHeading),
    LoadFinished,
}

#[derive(Clone, Debug)]
struct WebPaneModel {}

fn get_nav_state(webview: &WebView) -> NavigationState {
    NavigationState {
        can_go_back: webview.can_go_back(),
        can_go_forward: webview.can_go_forward(),
    }
}

#[relm4::component(pub, async)]
impl AsyncComponent for WebPaneModel {
    type Init = String;

    type Input = WebPaneMsg;
    type Output = TabMsg;

    type CommandOutput = Nothing;

    view! {
        #[name="webview"]
        webkit6::WebView {
            set_hexpand: true,
            set_vexpand: true,

            connect_uri_notify[sender] => move |webview| {
                let new_uri = webview.uri().map_or("".to_string(), |s| s.to_string());

                let _ = sender.output(TabMsg::UpdateURI(new_uri));
                sender.input(WebPaneMsg::UpdateNavState);
            },

            connect_title_notify[sender] => move |webview| {
                let new_title = webview.title().map(|s| s.to_string());

                let _ = sender.output(TabMsg::UpdateTitle(new_title));
            },

            connect_load_changed[sender] => move |_webview, event| {
                let _ = sender.output(TabMsg::UpdateLoadState(event));
                sender.input(WebPaneMsg::UpdateNavState);

                if event == LoadEvent::Finished { sender.input(WebPaneMsg::LoadFinished) }
            },

            connect_estimated_load_progress_notify[sender] => move |webview| {
                let _ = sender.output(TabMsg::UpdateLoadProgress(webview.estimated_load_progress()));
            },
        }
    }

    async fn init(
        uri: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let model = WebPaneModel {};

        let widgets = view_output!();

        let settings = webkit6::prelude::WebViewExt::settings(&widgets.webview).unwrap();
        settings.set_enable_developer_extras(true);

        let _ = &widgets.webview.connect_realize(move |webview| {
            webview.load_uri(&uri);
        });

        AsyncComponentParts { model, widgets }
    }

    async fn update_with_view(
        &mut self,
        widgets: &mut Self::Widgets,
        msg: Self::Input,
        sender: AsyncComponentSender<Self>,
        _root: &Self::Root,
    ) {
        println!("WebPaneModel: {:?}", msg);

        let webview = &widgets.webview;
        match msg {
            WebPaneMsg::UpdatedURI(new_uri) => {
                webview.load_uri(&new_uri);
                sender.input(WebPaneMsg::UpdateNavState);
            }
            WebPaneMsg::SelectedHeading(heading) => {
                let result = self.try_scroll_to_heading(webview, &heading).await;
                if let Err(e) = result {
                    eprintln!("Error scrolling to heading: {}", e);
                }
                sender.input(WebPaneMsg::UpdateNavState);
            }
            WebPaneMsg::GoBack => {
                webview.go_back();
                sender.input(WebPaneMsg::UpdateNavState);
            }
            WebPaneMsg::GoForward => {
                webview.go_forward();
                sender.input(WebPaneMsg::UpdateNavState);
            }
            WebPaneMsg::LoadFinished => {
                let headings = self.get_page_headings(webview).await;

                let _ = sender.output(TabMsg::UpdateOutline(match headings {
                    Ok(headings) => Some(headings),
                    Err(e) => {
                        eprintln!("Error getting headings for {:?}: {}", webview.uri(), e);
                        None
                    }
                }));

                sender.input(WebPaneMsg::UpdateNavState);
            },
            WebPaneMsg::UpdateNavState => {
                let _ = sender.output(TabMsg::UpdateNavState(get_nav_state(webview)));
            },
        }
    }
}

impl WebPaneModel {
    async fn get_page_headings(&self, webview: &WebView) -> Result<Outline, webkit6::glib::Error> {
        let script = /* js */ r#"
            let headings = Array.from(document.querySelectorAll('h1, h2, h3, h4, h5, h6'))

            globalThis.__headings = headings;

            return JSON.stringify(
                headings.map((elem, index) => ({
                    "tag_name": elem.localName,
                    "inner_text": elem.innerText,
                    "index": index,
                    "id": elem.id
                }))
            )
        "#;

        let result = webview
            .call_async_javascript_function_future(script, None, None, None)
            .await;

        result.map(|val| {
            let val_str = val.to_str().to_string();
            serde_json::from_str(&val_str).expect("JSON deserialization failed")
        })
    }

    async fn try_scroll_to_heading(
        &self,
        webview: &WebView,
        heading: &HTMLHeading,
    ) -> Result<(), webkit6::glib::Error> {
        let script = /* js */ r#"
            console.log(`Scrolling to heading with index: ${index}, id: ${id}`);

            if (id !== "") {
                const elem = document.getElementById(id);

                window.location.hash = '#' + id;

                if (elem)
                    elem.scrollIntoView();
                else
                    console.error("Element with id " + id + " not found");
            } else {
                const elem = globalThis.__headings[index];
                elem.scrollIntoView();
            }
        "#;

        let index = heading.index as u64;
        let id = heading.id.clone().unwrap_or_default();

        let args = VariantDict::new(None);

        args.insert("index", index);
        args.insert("id", id);

        let res = webview
            .call_async_javascript_function_future(script, Some(&args.end()), None, None)
            .await;

        res.map(|_| ())
    }
}

#[derive(Debug)]
struct TabModel {
    uri: String,
    web_pane: AsyncController<WebPaneModel>,
    current_title: Option<String>,
    progress_visible: bool,
    load_progress: f64,
    nav_state: NavigationState,
    outline: Option<Outline>,
}

#[derive(Debug)]
enum TabMsg {
    GoBack,
    GoForward,
    UpdateTitle(Option<String>),
    UpdatedURI(String),
    UpdateLoadState(LoadEvent),
    UpdateNavState(NavigationState),
    UpdateLoadProgress(f64),
    UpdateOutline(Option<Outline>),
    UpdateURI(String),
    SelectedHeading(HTMLHeading),
}

#[derive(Debug)]
enum TabResponse {
    SelectTab(DynamicIndex),
    UpdateOutline(Option<Outline>),
}

fn is_progress_visible(event: LoadEvent) -> bool {
    match event {
        LoadEvent::Started    => true,
        LoadEvent::Committed  => true,
        LoadEvent::Redirected => true,
        LoadEvent::Finished   => false,
        _                     => false,
    }
}

#[relm4::factory(async)]
impl AsyncFactoryComponent for TabModel {
    type Init = String;
    type Input = TabMsg;
    type Output = TabResponse;
    type CommandOutput = Nothing;
    type ParentWidget = adw::TabView;

    async fn init_model(
        uri: Self::Init,
        _index: &DynamicIndex,
        sender: AsyncFactorySender<Self>,
    ) -> Self {
        let web_pane = WebPaneModel::builder()
            .launch(uri.clone())
            .forward(sender.input_sender(), identity);

        Self {
            web_pane,
            uri,
            current_title: None,
            nav_state: Default::default(),
            load_progress: 0.0,
            progress_visible: false,
            outline: None,
        }
    }

    fn init_widgets(
        &mut self,
        index: &DynamicIndex,
        root: Self::Root,
        returned_widget: &<Self::ParentWidget as relm4::factory::FactoryView>::ReturnedWidget,
        sender: AsyncFactorySender<Self>,
    ) -> Self::Widgets {
        let widgets = view_output!();

        widgets
    }

    view! {
        #[root]
        adw::Bin {
            #[wrap(Some)]
            set_child = self.web_pane.widget(),
        },

        #[local_ref]
        returned_widget -> adw::TabPage {
            #[watch]
            set_title: &self.current_title.as_ref().map_or("(no title)", |s| s),

            connect_selected_notify[sender, index] => move |_tab_page| {
                sender.output(TabResponse::SelectTab(index.clone())).expect("Receiver does not exist");
            },
        }
    }

    async fn update(&mut self, msg: Self::Input, sender: AsyncFactorySender<Self>) {
        println!("TabModel: {:?}", msg);

        match msg {
            TabMsg::GoBack => {
                self.web_pane.emit(WebPaneMsg::GoBack);
            }
            TabMsg::GoForward => {
                self.web_pane.emit(WebPaneMsg::GoForward);
            }
            TabMsg::UpdateTitle(s) => {
                self.current_title = s.clone();
                NAV_BAR_BROKER.send(NavBarMsg::UpdatedTitle(s.clone()));
            }
            TabMsg::UpdateURI(uri) => {
                self.uri = uri.clone();
                NAV_BAR_BROKER.send(NavBarMsg::UpdatedURI(uri));
            }
            TabMsg::UpdateNavState(state) => {
                self.nav_state = state;
                NAV_BAR_BROKER.send(NavBarMsg::UpdatedNavState(state));
            }
            TabMsg::UpdateLoadState(event) => {
                self.progress_visible = is_progress_visible(event);
                NAV_BAR_BROKER.send(NavBarMsg::UpdatedProgressVisible(self.progress_visible));
            }
            TabMsg::UpdateLoadProgress(progress) => {
                self.load_progress = progress;
                NAV_BAR_BROKER.send(NavBarMsg::UpdatedLoadingProgress(progress));
            }
            TabMsg::UpdateOutline(outline) => {
                self.outline = outline.clone();
                OUTLINE_SIDEBAR_BROKER.send(OutlineSidebarMsg::UpdatedOutline(outline));
            }
            TabMsg::UpdatedURI(uri) => {
                self.uri = uri.clone();
                self.web_pane.emit(WebPaneMsg::UpdatedURI(uri));
            }
            TabMsg::SelectedHeading(heading) => {
                self.web_pane.emit(WebPaneMsg::SelectedHeading(heading));
            }
        }
    }
}

#[derive(Debug)]
struct OutlineItem {
    value: HTMLHeading,
}

struct OutlineItemWidgets {
    label: gtk::Label,
}

impl RelmListItem for OutlineItem {
    type Root = gtk::Box;

    type Widgets = OutlineItemWidgets;

    fn setup(_list_item: &gtk::ListItem) -> (Self::Root, Self::Widgets) {
        relm4::view! {
            root_box = gtk::Box {
                set_margin_horizontal: 2,

                #[name = "label"]
                gtk::Label { },
            },
        }

        let widgets = OutlineItemWidgets { label };

        (root_box, widgets)
    }

    fn bind(&mut self, widgets: &mut Self::Widgets, _root: &mut Self::Root) {
        let OutlineItemWidgets { label } = widgets;

        let margin_left: usize = 2 + self.value.indent_levels() * 10;

        label.set_label(&self.value.inner_text);
        label.set_margin_start(margin_left.try_into().unwrap_or(0));
    }
}

static OUTLINE_SIDEBAR_BROKER: MessageBroker<OutlineSidebarMsg> = MessageBroker::new();

#[derive(Debug)]
struct OutlineSidebarModel {
    outline: Option<Outline>,
    list_view_wrapper: TypedListView<OutlineItem, gtk::SingleSelection>,
}

#[derive(Debug)]
enum OutlineSidebarMsg {
    UpdatedOutline(Option<Outline>),
    SelectItem(u32),
}

#[derive(Debug)]
enum OutlineSidebarResponse {
    SelectHeading(HTMLHeading),
}

#[relm4::component(async)]
impl SimpleAsyncComponent for OutlineSidebarModel {
    type Init = Nothing;

    type Input = OutlineSidebarMsg;
    type Output = OutlineSidebarResponse;

    view! {
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &adw::WindowTitle {
                    set_title: "Outline",
                },

                set_decoration_layout: Some(""),
            },

            gtk::ScrolledWindow {
                #[wrap(Some)]
                set_child = match model.outline {
                    Some(_) => {
                        #[local_ref]
                        *list_view -> gtk::ListView { }
                    },
                    None => {
                        gtk::Label {
                            set_hexpand: true,
                            set_vexpand: true,
                            set_justify: gtk::Justification::Center,
                            set_valign:  gtk::Align::Center,

                            set_use_markup: true,

                            set_label: "<i>No outline available</i>",
                        }
                    }
                }
            },
        }
    }

    async fn init(
        _init: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let list_view_wrapper: TypedListView<OutlineItem, gtk::SingleSelection> =
            TypedListView::new();

        let sender_clone = sender.clone();

        list_view_wrapper.selection_model.connect_selection_changed(
            move |model, _position, _n_items| {
                sender_clone.input(OutlineSidebarMsg::SelectItem(model.selected()));
            },
        );

        let model = OutlineSidebarModel {
            outline: None,
            list_view_wrapper,
        };

        let list_view = &model.list_view_wrapper.view;

        let widgets = view_output!();

        AsyncComponentParts { model, widgets }
    }

    async fn update(&mut self, message: Self::Input, sender: AsyncComponentSender<Self>) -> () {
        println!("OutlineSidebarModel: {:?}", message);

        match message {
            OutlineSidebarMsg::UpdatedOutline(outline) => {
                self.list_view_wrapper.clear();

                if let Some(items) = &outline {
                    for value in items {
                        self.list_view_wrapper.append(OutlineItem {
                            value: value.clone(),
                        });
                    }
                }

                self.outline = outline;
            }

            OutlineSidebarMsg::SelectItem(index) => {
                if let Some(outline) = &self.outline {
                    match outline.get(index as usize) {
                        Some(heading) => {
                            let _ = sender
                                .output(OutlineSidebarResponse::SelectHeading(heading.clone()));
                        }
                        None => {
                            eprintln!("Invalid heading index: {}", index);
                        }
                    }
                } else {
                    unreachable!("Selected item without an outline")
                }
            }
        }
    }
}

static NAV_BAR_BROKER: MessageBroker<NavBarMsg> = MessageBroker::new();

#[tracker::track]
#[derive(Debug)]
struct NavBarModel {
    title: Option<String>,
    uri: String,
    nav_state: NavigationState,
    uri_editable: bool,
    sidebar_visible: bool,
    progress_visible: bool,
    load_progress: f64,
}

#[derive(Debug)]
enum NavBarMsg {
    StartEditingURI,
    CancelEditingURI,
    SetNewURI(String),
    UpdatedTitle(Option<String>),
    UpdatedSidebarVisibility(bool),
    UpdatedNavState(NavigationState),
    UpdatedURI(String),
    UpdatedProgressVisible(bool),
    UpdatedLoadingProgress(f64),
}

#[relm4::component(async)]
impl SimpleAsyncComponent for NavBarModel {
    type Init = String;
    type Input = NavBarMsg;
    type Output = AppMsg;

    view! {
        #[root]
        gtk::Box {
            set_orientation: Orientation::Vertical,

            adw::HeaderBar {
                set_hexpand: true,

                set_decoration_layout: Some(":close"),

                pack_start = &gtk::Box {
                    set_spacing: 5,

                    gtk::Box {
                        add_css_class: relm4::css::LINKED,

                        #[name="back"]
                        gtk::Button::from_icon_name("go-previous-symbolic") {
                            connect_clicked[sender] => move |_| {
                                let _ = sender.output(AppMsg::GoBack);
                            },

                            #[watch]
                            set_sensitive: model.nav_state.can_go_back,
                        },

                        #[name="forward"]
                        gtk::Button::from_icon_name("go-next-symbolic") {
                            connect_clicked[sender] => move |_| {
                                let _ = sender.output(AppMsg::GoForward);
                            },

                            #[watch]
                            set_sensitive: model.nav_state.can_go_forward,
                        },
                    },

                    gtk::Button::from_icon_name("bookmark-outline-symbolic") {
                        connect_clicked[sender] => move |_| {
                            //let _ = sender.output(AppMsg::Bookmark);
                        },
                    },
                },

                pack_end = &gtk::Box {
                    set_spacing: 5,

                    #[name="new_tab"]
                    gtk::Button::from_icon_name("tab-new-symbolic") {
                        connect_clicked[sender] => move |_| {
                            let _ = sender.output(AppMsg::NewTab);
                        },
                    },

                    #[name="sidebar_toggle"]
                    gtk::ToggleButton {
                        set_icon_name: "sidebar-show-right-symbolic",

                        #[watch]
                        set_active: model.sidebar_visible,

                        connect_clicked[sender] => move |button| {
                            let _ = sender.output(AppMsg::UpdateSidebarVisibility(button.is_active()));
                        },
                    },
                },

                set_show_title: true,

                #[wrap(Some)]
                set_title_widget = &adw::Bin {
                    if model.uri_editable {
                        #[name="uri_entry"]
                        gtk::Entry {
                            set_hexpand: true,
                            set_vexpand: true,

                            set_placeholder_text: Some("Enter address"),

                            set_activates_default: true,

                            #[watch]
                            #[block_signal(changed)]
                            set_text: &model.uri,

                            #[track = "model.changed(NavBarModel::uri_editable())"]
                            grab_focus: (),

                            connect_activate[sender] => move |entry| {
                                sender.input(NavBarMsg::SetNewURI(entry.text().to_string()))
                            } @changed,

                            connect_state_flags_changed[sender] => move |entry, old_flags| {
                                if old_flags.contains(gtk::StateFlags::FOCUSED)
                                        && !entry.state_flags().contains(gtk::StateFlags::FOCUSED) {
                                    sender.input(NavBarMsg::CancelEditingURI);
                                }
                            },
                        }
                    } else {
                        gtk::Button {
                            add_css_class: "flat",

                            adw::WindowTitle {
                                #[watch]
                                set_title: &model.title.as_ref().map_or("(no title)", |s| s),

                                #[watch]
                                set_subtitle: &model.uri,
                            },

                            connect_clicked => NavBarMsg::StartEditingURI,
                        }
                    },
                },

            },

            gtk::ProgressBar {
                set_hexpand: true,

                #[watch]
                set_visible: model.progress_visible,

                #[watch]
                set_fraction: model.load_progress,
            },
        }
    }

    async fn init(
        init: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let model = NavBarModel {
            title: None,
            uri: init,
            uri_editable: false,
            nav_state: Default::default(),
            sidebar_visible: true,
            progress_visible: false,
            load_progress: 0.0,
            tracker: Default::default(),
        };

        let widgets = view_output!();

        AsyncComponentParts { model, widgets }
    }

    async fn update(&mut self, msg: Self::Input, sender: AsyncComponentSender<Self>) {
        self.reset();

        println!("NavBarModel: {:?}", msg);

        match msg {
            NavBarMsg::StartEditingURI => {
                self.set_uri_editable(true);
            }
            NavBarMsg::CancelEditingURI => {
                self.set_uri_editable(false);
            }
            NavBarMsg::SetNewURI(uri) => {
                self.set_uri_editable(false);
                self.set_uri(uri.clone());
                let _ = sender.output(AppMsg::UpdateURI(uri));
            }
            NavBarMsg::UpdatedTitle(title) => {
                self.set_title(title.clone());
            }
            NavBarMsg::UpdatedSidebarVisibility(visible) => {
                self.set_sidebar_visible(visible);
            }
            NavBarMsg::UpdatedNavState(state) => {
                self.set_nav_state(state);
            }
            NavBarMsg::UpdatedProgressVisible(progress) => {
                self.set_progress_visible(progress);
            }
            NavBarMsg::UpdatedLoadingProgress(progress) => {
                self.set_load_progress(progress);
            }
            NavBarMsg::UpdatedURI(uri) => {
                self.set_uri(uri);
            }
        }
    }
}

#[derive(Debug)]
struct NavSidebar {}

#[derive(Debug)]
enum NavSidebarMsg {}

#[derive(Debug)]
enum NavSidebarResponse {}

#[relm4::component(async)]
impl SimpleAsyncComponent for NavSidebar {
    type Init = Nothing;
    type Input = NavSidebarMsg;
    type Output = NavSidebarResponse;

    view! {
        #[root]
        adw::ToolbarView {
            add_top_bar = &adw::HeaderBar {
                #[wrap(Some)]
                set_title_widget = &gtk::DropDown {
                    #[wrap(Some)]
                    set_model = &gtk::StringList::new(&[
                        "Man pages",
                        "Texinfo",
                        "HTML docs",
                    ]),
                },

                pack_start = &gtk::Box {
                    #[name="search_start"]
                    gtk::ToggleButton {
                        set_icon_name: "edit-find-symbolic",
                    },
                }
            },

            gtk::Box {
                set_orientation: Orientation::Vertical,
                set_spacing: 3,
                set_hexpand: true,
                set_align: gtk::Align::Fill,

                gtk::Label { set_label: "Nav entry 1" },
                gtk::Label { set_label: "Nav entry 2" },
                gtk::Label { set_label: "Nav entry 3" },
                gtk::Label { set_label: "Nav entry 4" },
                gtk::Label { set_label: "Nav entry 5" },
                gtk::Label { set_label: "Nav entry 6" },
            },
        }
    }

    async fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let model = NavSidebar {};
        let widgets = view_output!();
        AsyncComponentParts { model, widgets }
    }

    async fn update(&mut self, msg: Self::Input, _sender: AsyncComponentSender<Self>) {
        match msg {}
    }
}

#[derive(Debug)]
enum AppMsg {
    NewTab,
    GoBack,
    GoForward,
    UpdateSidebarVisibility(bool),
    SelectTab(DynamicIndex),
    SelectHeading(HTMLHeading),
    UpdateOutline(Option<Vec<HTMLHeading>>),
    UpdateURI(String),
}

#[derive(Debug)]
struct AppModel {
    starting_uri: String,
    tabs: AsyncFactoryVecDeque<TabModel>,
    nav_bar: AsyncController<NavBarModel>,
    nav_sidebar: AsyncController<NavSidebar>,
    current_tab: Option<DynamicIndex>,
    outline_sidebar: AsyncController<OutlineSidebarModel>,
    sidebar_visible: bool,
}

#[relm4::component(async)]
impl SimpleAsyncComponent for AppModel {
    type Init = String;

    type Input = AppMsg;
    type Output = Nothing;

    view! {
        #[root]
        adw::ApplicationWindow {
            set_title: Some("DocViewer"),
            set_default_size: (1024, 600),

            adw::NavigationSplitView {
                set_min_sidebar_width: 256.0,

                #[wrap(Some)]
                set_sidebar = &adw::NavigationPage {
                    set_title: "Navigation",

                    #[wrap(Some)]
                    set_child = model.nav_sidebar.widget(),
                },

                #[wrap(Some)]
                set_content = &adw::NavigationPage {
                    #[watch]
                    set_title: "DocViewer",

                    adw::ToolbarView {
                        add_top_bar = &gtk::Box {
                            set_hexpand: true,
                            set_orientation: Orientation::Vertical,

                            append = model.nav_bar.widget(),

                            adw::TabBar {
                                set_expand_tabs: true,

                                #[watch]
                                set_view: Some(&model.tabs.widget()),
                            },
                        },

                        adw::OverlaySplitView {
                            set_sidebar_position: gtk::PackType::End,

                            #[wrap(Some)]
                            set_content = model.tabs.widget(),

                            #[wrap(Some)]
                            set_sidebar = model.outline_sidebar.widget(),

                            #[watch]
                            set_show_sidebar: model.sidebar_visible,
                        },
                    },
                },
            },
        }
    }

    async fn init(
        starting_uri: Self::Init,
        root: Self::Root,
        sender: AsyncComponentSender<Self>,
    ) -> AsyncComponentParts<Self> {
        let mut tabs = AsyncFactoryVecDeque::builder()
            .launch(adw::TabView::default())
            .forward(sender.input_sender(), |msg| match msg {
                TabResponse::SelectTab(i) => AppMsg::SelectTab(i),
                TabResponse::UpdateOutline(o) => AppMsg::UpdateOutline(o),
            });

        let initial_tab = tabs.guard().push_back(starting_uri.clone());

        let nav_bar = NavBarModel::builder()
            .launch_with_broker(starting_uri.clone(), &NAV_BAR_BROKER)
            .forward(sender.input_sender(), identity);

        let nav_sidebar = NavSidebar::builder()
            .launch(())
            .forward(sender.input_sender(), |_| unimplemented!());

        let outline_sidebar = OutlineSidebarModel::builder()
            .launch_with_broker((), &OUTLINE_SIDEBAR_BROKER)
            .forward(sender.input_sender(), |msg| match msg {
                OutlineSidebarResponse::SelectHeading(heading) => AppMsg::SelectHeading(heading),
            });

        let model = AppModel {
            starting_uri,
            tabs,
            nav_bar,
            nav_sidebar,
            outline_sidebar,
            current_tab: Some(initial_tab),
            sidebar_visible: true,
        };

        let widgets = view_output!();

        AsyncComponentParts { model, widgets }
    }

    async fn update(&mut self, msg: Self::Input, sender: AsyncComponentSender<Self>) {
        println!("AppModel: {:?}", msg);

        match msg {
            AppMsg::NewTab => {
                self.tabs.guard().push_back(self.starting_uri.clone());
            }
            AppMsg::GoBack => {
                self.send_to_current_tab(TabMsg::GoBack);
            }
            AppMsg::GoForward => {
                self.send_to_current_tab(TabMsg::GoForward);
            }
            AppMsg::UpdateSidebarVisibility(visible) => {
                self.sidebar_visible = visible;
            }
            AppMsg::SelectTab(index) => {
                self.current_tab = Some(index);

                let cur_tab = self.get_current_tab().unwrap();
                sender.input(AppMsg::UpdateOutline(cur_tab.outline.clone()));
            }
            AppMsg::SelectHeading(heading) => {
                self.send_to_current_tab(TabMsg::SelectedHeading(heading));
            }
            AppMsg::UpdateOutline(outline) => {
                self.outline_sidebar
                    .emit(OutlineSidebarMsg::UpdatedOutline(outline));
            }
            AppMsg::UpdateURI(uri) => {
                self.send_to_current_tab(TabMsg::UpdatedURI(uri));
            }
        }
    }
}

impl AppModel {
    fn send_to_current_tab(&self, msg: <TabModel as AsyncFactoryComponent>::Input) {
        let cur_index = self.current_tab.as_ref().map(|i| i.current_index());
        self.tabs.send(cur_index.expect("No current tab"), msg);
    }

    fn get_current_tab(&self) -> Option<&TabModel> {
        self.current_tab
            .as_ref()
            .and_then(|index| self.tabs.get(index.current_index()))
    }
}

relm4::new_action_group!(WindowActionGroup, "win");

static STYLESHEET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/share/app.css");

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_path(STYLESHEET_PATH);

    let display = Display::default().expect("Failed to get default display");

    let priority = gtk::STYLE_PROVIDER_PRIORITY_APPLICATION;

    gtk::style_context_add_provider_for_display(&display, &provider, priority);
}

fn main() {
    let app = adw::Application::new(Some("dev.ap5.docviewer"), Default::default());

    app.connect_startup(|_| load_css());

    let starting_uri = "file:///tmp/man.html";

    let relm_app = RelmApp::from_app(app);
    relm_app.run_async::<AppModel>(starting_uri.to_string());
}
