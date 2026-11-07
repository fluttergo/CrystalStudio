using System.IO;

namespace CrystalStudio;

internal sealed class ObjectExtractionWindow : EditorWindow
{
    private readonly Func<ExtractionInput> current;
    private readonly Action saved;
    private readonly string indexPath;
    private readonly AssetSourceFormat assetFormat;
    private readonly Func<AiSettings> aiSettings;
    private readonly AiClient aiClient;
    private readonly ComboBox scope=new(){ItemsSource=new[]{"Current map","Current selection","Map directory","Existing categories"},SelectedIndex=0,MinWidth=150};
    private readonly TextBox directory=new(){Watermark=LocalText.T("Map directory"),MinWidth=180};
    private readonly TextBox resource=new(){Watermark=LocalText.T("Resource folder"),MinWidth=180};
    private readonly TextBox search=new(){Watermark=LocalText.T("Search candidates or source"),MinWidth=170};
    private readonly ComboBox state=new(){ItemsSource=new[]{"All candidates","Unreviewed","Reviewed","Existing objects","New objects","Selection boundary"},SelectedIndex=0,MinWidth=140};
    private readonly TextBlock status=new(){TextWrapping=TextWrapping.Wrap};
    private readonly TextBlock statistics=new(){TextWrapping=TextWrapping.Wrap};
    private readonly TextBlock pageLabel=new(){VerticalAlignment=VerticalAlignment.Center};
    private readonly ProgressBar progress=new(){Height=4,Minimum=0,Maximum=100};
    private readonly ListBox cards=new(){SelectionMode=SelectionMode.Multiple,AutoScrollToSelectedItem=false};
    private readonly TabControl categories=new(){TabStripPlacement=Dock.Left};
    private readonly StackPanel details=new(){Spacing=6};
    private readonly Button scan,save,cancel,undo,redo,previous,next,aiSelected,aiUnreviewed;
    private readonly List<Control> mutationControls=[];
    private readonly SemaphoreSlim previewSlots=new(2);
    private readonly HashSet<Task> previewTasks=[];
    private readonly Dictionary<Control,RasterImage> previewImages=[];
    private CancellationTokenSource previewStop=new(),operation=new();
    private AssetStore? assets;
    private ExtractionSession? session;
    internal ExtractionReview? Review {get;private set;}
    private bool running,closed,refreshing;
    private int page;
    private const int PageSize=48;
    private Point extractionDragStart;
    private ExtractionCandidate? extractionDragCandidate;
    internal ObjectExtractionWindow(Func<ExtractionInput> current,string? resourceFolder,string indexPath,Action saved,
        AssetSourceFormat assetFormat = AssetSourceFormat.Default, Func<AiSettings>? aiSettings = null, AiClient? aiClient = null)
    {
        this.current=current;this.indexPath=indexPath;this.saved=saved;this.assetFormat=assetFormat;
        this.aiSettings=aiSettings??(()=>new SettingsStore().Load().Ai??new());this.aiClient=aiClient??new();
        StudioLog.Info("Extraction.Window",$"Opened; Resources={resourceFolder}; Format={assetFormat}; Catalogue={indexPath}");
        Title=LocalText.T("Extract Map Objects");Width=1420;Height=900;MinWidth=1120;MinHeight=700;WindowStartupLocation=WindowStartupLocation.CenterOwner;
        resource.Text=resourceFolder;
        scope.Name="ExtractionScope";directory.Name="ExtractionDirectory";
        var root=new Grid{Margin=new(16),RowDefinitions=new("Auto,Auto,Auto,*,Auto"),RowSpacing=8};Content=root;
        var inputs=new Grid{ColumnDefinitions=new("Auto,*,Auto,*,Auto,Auto"),ColumnSpacing=8};root.Children.Add(inputs);
        inputs.Children.Add(scope);Grid.SetColumn(directory,1);inputs.Children.Add(directory);
        var chooseMaps=Button("Choose maps…",()=>ChooseFolder(directory));Grid.SetColumn(chooseMaps,2);inputs.Children.Add(chooseMaps);
        Grid.SetColumn(resource,3);inputs.Children.Add(resource);
        var chooseAssets=Button("Choose resources…",()=>ChooseFolder(resource));Grid.SetColumn(chooseAssets,4);inputs.Children.Add(chooseAssets);
        scan=Button("Scan",()=>_ = ScanAsync());scan.Name="ExtractionScan";Grid.SetColumn(scan,5);inputs.Children.Add(scan);
        var summary=new StackPanel{Spacing=5};summary.Children.Add(new TextBlock{Text=LocalText.T("Review object boundaries and collision before saving. Source maps remain unchanged."),TextWrapping=TextWrapping.Wrap});
        summary.Children.Add(statistics);summary.Children.Add(progress);Grid.SetRow(summary,1);root.Children.Add(summary);
        var filters=new StackPanel{Orientation=Orientation.Horizontal,Spacing=8};filters.Children.Add(search);filters.Children.Add(state);
        previous=Button("Previous",()=>{page--;RefreshCards();});next=Button("Next",()=>{page++;RefreshCards();});filters.Children.Add(previous);filters.Children.Add(pageLabel);filters.Children.Add(next);
        Grid.SetRow(filters,2);root.Children.Add(filters);
        var body=new Grid{ColumnDefinitions=new("156,*,480"),ColumnSpacing=10};Grid.SetRow(body,3);root.Children.Add(body);
        categories.SetResource(ThemeProperty,"ExtractionCategoryTabs");categories.SetResource(ItemsControl.ItemContainerThemeProperty,"CategoryTabItem");
        categories.Name="ExtractionCategories";cards.Name="ExtractionCandidates";cards.SetResource(ItemsControl.ItemContainerThemeProperty,"CategoryCard");
        body.Children.Add(categories);Grid.SetColumn(cards,1);body.Children.Add(cards);
        cards.ItemsPanel=new FuncTemplate<Panel?>(()=>new WrapPanel());
        cards.SetValue(ScrollViewer.HorizontalScrollBarVisibilityProperty,ScrollBarVisibility.Disabled);
        cards.ItemTemplate=new FuncDataTemplate<ExtractionCandidate>((c,_)=>c==null?null:Card(c));
        var detailScroll=new ScrollViewer{Content=details,HorizontalScrollBarVisibility=ScrollBarVisibility.Disabled};Grid.SetColumn(detailScroll,2);body.Children.Add(detailScroll);
        var footer=new Grid{ColumnDefinitions=new("*,Auto"),ColumnSpacing=12};Grid.SetRow(footer,4);root.Children.Add(footer);footer.Children.Add(status);
        var actions=new StackPanel{Orientation=Orientation.Horizontal,Spacing=6};Grid.SetColumn(actions,1);footer.Children.Add(actions);
        undo=Button("Undo",()=>Command(()=>Review!.Undo()));redo=Button("Redo",()=>Command(()=>Review!.Redo()));
        var review=Button("Review selected",()=>Command(()=>Review!.Update(SelectedIds(),reviewed:true)));
        review.Name="ExtractionReviewSelected";
        var unreview=Button("Mark unreviewed",()=>Command(()=>Review!.Update(SelectedIds(),reviewed:false)));
        var merge=Button("Merge selected",()=>Command(()=>Review!.Merge(SelectedIds())));
        var exclude=Button("Exclude selected",()=>Command(()=>Review!.Exclude(SelectedIds())));
        var reviewActions=new WrapPanel{Margin=new(0,4)};foreach(var b in new[]{review,unreview,merge,exclude}){reviewActions.Children.Add(b);mutationControls.Add(b);}
        aiSelected=Button("AI identify / filter selected",()=>_ = RecognizeAsync());aiSelected.Name="ExtractionAiSelected";
        aiUnreviewed=Button("AI identify / filter all unreviewed",()=>_ = RecognizeAsync(true));aiUnreviewed.Name="ExtractionAiUnreviewed";
        reviewActions.Children.Add(aiSelected);reviewActions.Children.Add(aiUnreviewed);
        ToolTip.SetTip(aiSelected,LocalText.T("Uses Vision AI from Settings. Incomplete objects are excluded; uncertain results remain for review. Undo restores the whole batch."));
        ToolTip.SetTip(aiUnreviewed,LocalText.T("Uses Vision AI from Settings. Incomplete objects are excluded; uncertain results remain for review. Undo restores the whole batch."));
        // Review commands stay above the grid; the bottom row retains scan/save progress.
        filters.Children.Add(undo);filters.Children.Add(redo);
        var left=new Grid{RowDefinitions=new("*,Auto")};body.Children.Remove(cards);left.Children.Add(cards);Grid.SetRow(reviewActions,1);left.Children.Add(reviewActions);Grid.SetColumn(left,1);body.Children.Add(left);
        cancel=Button("Cancel",()=>operation.Cancel());save=Button("Save reviewed to Palette",()=>_ = SaveAsync());save.Name="ExtractionSave";
        cancel.Name="ExtractionCancel";
        save.SetResource(ThemeProperty,"PrimaryButton");actions.Children.Add(cancel);actions.Children.Add(save);
        search.TextChanged+=(_,_)=>{page=0;RefreshCards();};state.SelectionChanged+=(_,_)=>{page=0;RefreshCards();};
        categories.SelectionChanged+=(_,_)=>{if(!refreshing){page=0;RefreshCards();}};
        cards.SelectionChanged+=(_,_)=>{ShowDetails();UpdateState();};
        scope.SelectionChanged+=(_,_)=>{directory.IsEnabled=scope.SelectedIndex==2&&!running;};directory.IsEnabled=false;
        Closing+=(_,_)=>{StudioLog.Info("Extraction.Window","Closing; cancelling scan and previews");closed=true;operation.Cancel();previewStop.Cancel();};
        Closed+=async(_,_)=>{while(running)await Task.Delay(20);await ClearPreviews();assets?.Dispose();operation.Dispose();previewStop.Dispose();previewSlots.Dispose();};
        UpdateState();RefreshCategories();
    }
    private static Button Button(string text,Action action)
    {var button=new Button{Content=LocalText.T(text),Padding=new(9,5)};button.Click+=(_,_)=>action();return button;}
    private void ChooseFolder(TextBox target)
    {
        var picker=new OpenFolderDialog{Title=LocalText.T("Choose Folder…"),InitialDirectory=target.Text??Environment.CurrentDirectory};
        if(picker.ShowDialog(this)==true)target.Text=picker.FolderName;
    }
    private IEnumerable<Guid> SelectedIds()=>cards.SelectedItems?.OfType<ExtractionCandidate>().Select(c=>c.Id).ToArray()??[];
    private void Command(Action action)
    {
        if(Review==null||running)return;
        var ids=SelectedIds().ToHashSet();
        try{action();RefreshCategories(ids);status.Text=LocalText.T("Review updated. Use Undo to restore the previous review.");}
        catch(Exception ex){StudioLog.Error("Extraction.Review",ex);status.Text=LocalText.T(ex.Message);}
    }
    private void UpdateState()
    {
        scan.IsEnabled=scope.IsEnabled=resource.IsEnabled=!running;directory.IsEnabled=!running&&scope.SelectedIndex==2;
        cancel.IsEnabled=running;save.IsEnabled=!running&&Review?.Candidates.Any(c=>c.CanExport)==true;
        undo.IsEnabled=!running&&Review?.CanUndo==true;redo.IsEnabled=!running&&Review?.CanRedo==true;
        foreach(var control in mutationControls)control.IsEnabled=!running;
        aiSelected.IsEnabled=!running&&assets!=null&&SelectedIds().Any();
        aiUnreviewed.IsEnabled=!running&&assets!=null&&Review?.Candidates.Any(c=>!c.Reviewed)==true;
        statistics.Text=Review==null?LocalText.T("No scan results."):LocalText.Format("{0:N0} existing · {1:N0} new · {2:N0} duplicates · {3:N0} unreviewed · {4:N0} ready",
            Review.Candidates.Count(c=>c.Kind!=ExtractionKind.Discovered),Review.Candidates.Count(c=>c.Kind==ExtractionKind.Discovered),
            Review.Candidates.Sum(c=>c.Occurrences.Count-1),Review.Candidates.Count(c=>!c.Reviewed),Review.Candidates.Count(c=>c.CanExport));
    }
    internal async Task RecognizeAsync(bool allUnreviewed=false)
    {
        if(running||Review==null||assets==null)return;
        var selected=SelectedIds().ToHashSet();
        var targets=Review.Candidates.Where(c=>allUnreviewed?!c.Reviewed:selected.Contains(c.Id)).ToArray();
        if(targets.Length==0){status.Text=LocalText.T("Select candidates for AI recognition first.");return;}
        try
        {
            var endpoint=aiSettings().For(AiRole.Vision);endpoint.Validate(AiRole.Vision);
            var allowed=StampCategories.Read(indexPath).Objects.Select(c=>c.Type).OfType<string>()
                .Concat(Review.Candidates.Select(c=>c.Category)).Append("Unclassified")
                .Where(c=>!string.IsNullOrWhiteSpace(c)).Distinct(StringComparer.OrdinalIgnoreCase).OrderBy(c=>c).ToArray();
            running=true;operation.Dispose();operation=new();UpdateState();details.IsEnabled=false;
            progress.IsIndeterminate=false;progress.Value=0;
            var results=new Dictionary<Guid,AiObjectRecognition>();
            StudioLog.Info("Extraction.AI",$"Starting; Prompt={ExtractionAi.PromptVersion}; Targets={targets.Length}");
            foreach(var candidate in targets)
            {
                operation.Token.ThrowIfCancellationRequested();
                status.Text=LocalText.Format("AI recognizing {0} / {1}: {2}",results.Count+1,targets.Length,candidate.Name);
                var result=await ExtractionAi.RecognizeAsync(aiClient,endpoint,candidate,assets,allowed,operation.Token);
                results.Add(candidate.Id,result);
                StudioLog.Info("Extraction.AI",$"Result; Candidate={candidate.Id}; Name={result.Name}; Completeness={result.Completeness}; Category={result.Category}; Reason={result.Reason}");
                progress.Value=100.0*results.Count/targets.Length;
            }
            operation.Token.ThrowIfCancellationRequested();if(closed)return;
            Review.ApplyAi(results);RefreshCategories(selected);
            int excluded=results.Values.Count(r=>r.Completeness==AiObjectCompleteness.Incomplete);
            int uncertain=results.Values.Count(r=>r.Completeness==AiObjectCompleteness.Uncertain);
            StudioLog.Info("Extraction.AI",$"Applied; Candidates={results.Count}; Excluded={excluded}; Uncertain={uncertain}");
            status.Text=LocalText.Format("AI checked {0} objects: {1} incomplete excluded, {2} uncertain kept. Review remaining objects before saving; Undo restores the whole batch.",results.Count,excluded,uncertain);
        }
        catch(OperationCanceledException){status.Text=LocalText.T("AI recognition cancelled. Candidates unchanged.");}
        catch(Exception ex){StudioLog.Error("Extraction.AI",ex);status.Text=LocalText.Format("AI recognition failed. Candidates unchanged. {0}",LocalText.T(ex.Message));}
        finally{running=false;details.IsEnabled=true;UpdateState();}
    }
    internal async Task ScanAsync()
    {
        if(running)return;
        try
        {
            StudioLog.Info("Extraction.Scan",$"Starting; Scope={scope.SelectedIndex}; Resources={resource.Text}; MapDirectory={directory.Text}; Format={assetFormat}");
            bool catalogue = scope.SelectedIndex == 3;
            if(!catalogue && string.IsNullOrWhiteSpace(resource.Text))throw new InvalidDataException("Choose a resource folder.");
            IEnumerable<ExtractionInput> inputs = [];
            if(catalogue) status.Text=LocalText.T("Reading existing categories…");
            else if(scope.SelectedIndex==2)
            {
                if(!Directory.Exists(directory.Text))throw new DirectoryNotFoundException(LocalText.T("Choose a map directory."));
                string folder=Path.GetFullPath(directory.Text!);
                inputs=Directory.EnumerateFiles(folder,"*.map",new EnumerationOptions{RecurseSubdirectories=true,IgnoreInaccessible=true,MatchCasing=MatchCasing.CaseInsensitive}).Select(p=>new ExtractionInput(p));
            }
            else
            {
                status.Text=LocalText.T("Creating map snapshot…");
                var input=current();if(scope.SelectedIndex==1&&input.Selection==null)throw new InvalidDataException("Select a map area first.");
                inputs=[input with{Snapshot=ExtractionContent.Snapshot(input.Snapshot!),Selection=scope.SelectedIndex==1?input.Selection:null}];
            }
            running=true;operation.Dispose();operation=new();UpdateState();details.IsEnabled=false;progress.IsIndeterminate=true;
            Review=null;session=null;await ClearPreviews();RefreshCategories();assets?.Dispose();assets=null;string folderPath=resource.Text??"";
            var updates=new Progress<ExtractionProgress>(p=>{if(!closed)status.Text=LocalText.Format("{0} · {1:N0} maps · {2:N0} candidates · {3:N0} occurrences\n{4}",LocalText.T(p.Phase),p.Maps,p.Candidates,p.Occurrences,p.Source);});
            // Catalogue mode can list the existing catalogue even without a client resource
            // folder; previews are simply skipped until one is configured.
            if(!catalogue||!string.IsNullOrWhiteSpace(folderPath))
                assets=await Task.Run(()=>AssetStore.Load(folderPath,assetFormat),operation.Token);
            ExtractionSession result;
            if(catalogue)
                result=await Task.Run(()=>ObjectExtractionCatalogue.Load(indexPath,operation.Token,updates),operation.Token);
            else
            {
                var service=new ObjectExtractionService(indexPath,assets!);
                result=await service.ScanAsync(inputs,operation.Token,updates);
            }
            if(closed)return;session=result;Review=new(result.Candidates);page=0;RefreshCategories();
            StudioLog.Info("Extraction.Scan",$"Completed; Maps={result.Sources.Count}; Candidates={result.Candidates.Count}; Warnings={result.Warnings.Count}");
            foreach(var warning in result.Warnings)StudioLog.Warning("Extraction.Scan",warning);
            status.Text=LocalText.Format("Scan complete · {0:N0} maps · {1:N0} warnings. Select candidates to review.",result.Sources.Count,result.Warnings.Count);
            ToolTip.SetTip(status,string.Join("\n",result.Warnings.Take(30)));
        }
        catch(OperationCanceledException){StudioLog.Info("Extraction.Scan","Cancelled");status.Text=LocalText.T("Scan cancelled. Source maps remain unchanged.");}
        catch(Exception ex){StudioLog.Error("Extraction.Scan",ex,$"Resources={resource.Text}; MapDirectory={directory.Text}");status.Text=LocalText.T(ex.Message);}
        finally{running=false;details.IsEnabled=true;progress.IsIndeterminate=false;UpdateState();}
    }
    private async Task SaveAsync()
    {
        if(running||Review==null||session==null)return;
        running=true;operation.Dispose();operation=new();UpdateState();details.IsEnabled=false;progress.IsIndeterminate=true;status.Text=LocalText.T("Saving reviewed objects…");
        try
        {
            StudioLog.Info("Extraction.Export",$"Starting; Catalogue={indexPath}; Candidates={Review.Candidates.Count}");
            var result=await ObjectExtractionExport.SaveAsync(indexPath,session,Review.Candidates,operation.Token);
            StudioLog.Info("Extraction.Export",$"Completed; Objects={result.Objects.Count}; Report={result.ReportPath}");
            saved();status.Text=LocalText.Format("Saved {0:N0} unique objects. Report: {1}",result.Objects.Count,result.ReportPath);ToolTip.SetTip(status,result.ReportPath);
        }
        catch(OperationCanceledException){StudioLog.Info("Extraction.Export","Cancelled");status.Text=LocalText.T("Export cancelled. Catalogue unchanged.");}
        catch(Exception ex){StudioLog.Error("Extraction.Export",ex,$"Catalogue={indexPath}");status.Text=LocalText.T(ex.Message);}
        finally{running=false;details.IsEnabled=true;progress.IsIndeterminate=false;UpdateState();}
    }
    private void RefreshCategories(HashSet<Guid>? ids=null)
    {
        string selected=(categories.SelectedItem as TabItem)?.Tag as string??"All categories";refreshing=true;
        var values=new[]{"All categories"}.Concat((Review?.Candidates??[]).Select(c=>c.Category).Distinct().OrderBy(c=>c)).ToList();
        categories.ItemsSource=values.Select(c=>
        {
            int count=Review?.Candidates.Count(v=>c=="All categories"||v.Category==c)??0;
            var tab=new TabItem{Header=new TextBlock{Text=LocalText.T(c)+$" ({count})",TextWrapping=TextWrapping.Wrap},Tag=c};
            Avalonia.Automation.AutomationProperties.SetName(tab,LocalText.T(c));ToolTip.SetTip(tab,LocalText.T(c));return tab;
        }).ToArray();
        categories.SelectedIndex=Math.Max(0,values.IndexOf(selected));refreshing=false;RefreshCards(ids);
    }
    private void RefreshCards(HashSet<Guid>? selected=null)
    {
        if(cards==null)return;
        string category=(categories.SelectedItem as TabItem)?.Tag as string??"All categories",query=search.Text?.Trim()??"";
        var filtered=(Review?.Candidates??[]).Where(c=>(category=="All categories"||c.Category==category)&&
            (query.Length==0||string.Join(' ',c.Name,c.Category,string.Join(' ',c.Occurrences.Select(o=>o.Source))).Contains(query,StringComparison.OrdinalIgnoreCase))&&
            (state.SelectedIndex switch{1=>!c.Reviewed,2=>c.Reviewed,3=>c.Kind!=ExtractionKind.Discovered,4=>c.Kind==ExtractionKind.Discovered,5=>c.Occurrences.Any(o=>o.CrossesSelection),_=>true})).ToList();
        int pages=Math.Max(1,(filtered.Count+PageSize-1)/PageSize);page=Math.Clamp(page,0,pages-1);
        cards.ItemsSource=filtered.Skip(page*PageSize).Take(PageSize).ToArray();
        if(selected!=null)foreach(var c in cards.ItemsSource.OfType<ExtractionCandidate>().Where(c=>selected.Contains(c.Id)))cards.SelectedItems!.Add(c);
        pageLabel.Text=$"{page+1} / {pages}";previous.IsEnabled=page>0;next.IsEnabled=page+1<pages;
        ShowDetails();UpdateState();
    }
    private Control Card(ExtractionCandidate c)
    {
        var panel=new StackPanel{Width=104,Spacing=4,Margin=new(3)};ToolTip.SetTip(panel,c.Name+"\n"+c.Description);
        var image=new Image{Height=80,Stretch=Stretch.Uniform};panel.Children.Add(image);
        panel.Children.Add(new TextBlock{Text=(c.Reviewed?"✓ ":"")+c.Caption,FontSize=11,TextAlignment=TextAlignment.Center,TextTrimming=TextTrimming.CharacterEllipsis});
        bool requested=false;image.EffectiveViewportChanged+=(_,e)=>
        {
            if(!requested&&!closed&&image.IsEffectivelyVisible&&e.EffectiveViewport.Intersects(new Rect(image.Bounds.Size)))
            {requested=true;Track(Thumbnail(image,c));}
        };
        // Drag source: a left-click drag carries the candidate onto the open map so it can
        // be composed into a new map (drop handled in MainWindow.SetCompositions).
        panel.Cursor=new Cursor(StandardCursorType.Hand);
        panel.PointerPressed+=(_,e)=>
        {
            if(e.GetCurrentPoint(panel).Properties.IsLeftButtonPressed){extractionDragStart=e.GetPosition(panel);extractionDragCandidate=c;}
        };
        panel.PointerMoved+=(_,e)=>
        {
            if(extractionDragCandidate==null)return;
            if(!e.GetCurrentPoint(panel).Properties.IsLeftButtonPressed){extractionDragCandidate=null;return;}
            var pos=e.GetPosition(panel);
            if(Math.Abs(pos.X-extractionDragStart.X)>4||Math.Abs(pos.Y-extractionDragStart.Y)>4)
            {
                var payload=extractionDragCandidate;extractionDragCandidate=null;
                var data=new DataObject();data.Set(ExtractionDrag.Candidate,payload);
                _=DragDrop.DoDragDrop(e,data,DragDropEffects.Copy);
            }
        };
        panel.PointerReleased+=(_,_)=>{extractionDragCandidate=null;};
        panel.PointerCaptureLost+=(_,_)=>{extractionDragCandidate=null;};
        return panel;
    }
    private void Track(Task task){previewTasks.Add(task);_ = task.ContinueWith(_=>Dispatcher.UIThread.Post(()=>previewTasks.Remove(task)),TaskScheduler.Default);}
    private async Task Thumbnail(Image target,ExtractionCandidate c)
    {
        using var lifetime=CancellationTokenSource.CreateLinkedTokenSource(previewStop.Token);
        void Detached(object? s,VisualTreeAttachmentEventArgs e)=>lifetime.Cancel();
        target.DetachedFromVisualTree+=Detached;
        var token=lifetime.Token;var store=assets;if(store==null){target.DetachedFromVisualTree-=Detached;return;}
        bool entered=false;
        try
        {
            await previewSlots.WaitAsync(token);entered=true;
            var bitmap=await Task.Run(()=>StampPreview.Create(c.Cells,store,160,112,cancellation:token,waitForAssets:true,fastRaster:true),token);
            if(bitmap==null)return;if(token.IsCancellationRequested||closed||target.GetVisualRoot()==null){bitmap.Dispose();return;}
            HoldPreview(target,bitmap);target.Source=bitmap;
        }
        catch(OperationCanceledException){}
        catch(Exception ex){StudioLog.Error("Extraction.Thumbnail",ex,$"Candidate={c.Id}; Name={c.Name}; Cells={c.Cells.Count}");if(!closed)ToolTip.SetTip(target,ex.Message);}
        finally{target.DetachedFromVisualTree-=Detached;if(entered)previewSlots.Release();}
    }
    private void HoldPreview(Control target,RasterImage bitmap)
    {
        previewImages[target]=bitmap;
        void Release(object? s,VisualTreeAttachmentEventArgs e)
        {
            target.DetachedFromVisualTree-=Release;
            if(target is Image image)image.Source=null;
            if(target is ExtractionPreview preview)preview.Image=null;
            if(previewImages.Remove(target,out var old))old.Dispose();
        }
        target.DetachedFromVisualTree+=Release;
    }
    private async Task ClearPreviews()
    {
        previewStop.Cancel();await Task.WhenAll(previewTasks.ToArray());
        cards.ItemsSource=null;details.Children.Clear();foreach(var image in previewImages.Values)image.Dispose();previewImages.Clear();
        previewStop.Dispose();previewStop=new();
    }
    private void ShowDetails()
    {
        details.Children.Clear();var selected=cards.SelectedItems?.OfType<ExtractionCandidate>().ToList()??[];
        if(selected.Count==0){details.Children.Add(new TextBlock{Text=LocalText.T("Select a candidate to inspect its fragments, collision and sources."),TextWrapping=TextWrapping.Wrap});return;}
        var c=selected[0];
        StudioLog.Info("Extraction.Selection",$"Selected={selected.Count}; Candidate={c.Id}; Name={c.Name}; Cells={c.Cells.Count}; Sources={string.Join(", ",c.Occurrences.Take(5).Select(o=>o.Source))}");
        details.Children.Add(new TextBlock{Text=LocalText.Format("{0:N0} selected",selected.Count),FontWeight=FontWeight.SemiBold});
        var name=new TextBox{Text=selected.Count==1?c.Name:"",Watermark=LocalText.T("Name (leave empty to retain)")};details.Children.Add(name);
        var category=new TextBox{Text=c.Category,Watermark=LocalText.T("Category")};details.Children.Add(category);
        var boundary=new CheckBox{Content=LocalText.T("Allow complete objects crossing the selection"),IsChecked=c.IncludeBoundary};details.Children.Add(boundary);
        details.Children.Add(Button("Apply name / category / boundary",()=>Command(()=>Review!.Update(selected.Select(v=>v.Id),string.IsNullOrWhiteSpace(name.Text)?null:name.Text,category.Text,boundary:boundary.IsChecked==true))));
        if(selected.Count>1)return;
        details.Children.Add(new TextBlock{Text=c.Description,TextWrapping=TextWrapping.Wrap,FontSize=11});
        var area=new Grid{ColumnDefinitions=new("*,*"),ColumnSpacing=6};details.Children.Add(area);
        var isolated=new ExtractionPreview{Height=190};var context=new ExtractionPreview{Height=190};
        var a=new StackPanel();a.Children.Add(new TextBlock{Text=LocalText.T("Isolated object")});a.Children.Add(isolated);area.Children.Add(a);
        var b=new StackPanel();b.Children.Add(new TextBlock{Text=LocalText.T("Map context")});b.Children.Add(context);Grid.SetColumn(b,1);area.Children.Add(b);
        if(assets!=null){Track(DetailPreview(isolated,c,false));Track(DetailPreview(context,c,true));}
        var fragments=new ListBox{Name="ExtractionFragments",Height=150,SelectionMode=SelectionMode.Multiple};
        var rows=c.Cells.Where(s=>s.Cell.HasForeground).Select(s=>new FragmentRow(s.X,s.Y,$"({s.X}, {s.Y})  {s.Cell.Foreground!.Library}/{s.Cell.Foreground.Image&0x7FFF}  [{s.Cell.Foreground.Frames}, {s.Cell.Foreground.Tick}]")).ToArray();
        // Avalonia clears template data while recycling a virtualized row.
        fragments.ItemsSource=rows;fragments.ItemTemplate=new FuncDataTemplate<FragmentRow>((r,_)=>new TextBlock{Text=r?.Text,FontSize=11});details.Children.Add(fragments);
        void UpdateFragments(){var points=fragments.SelectedItems!.OfType<FragmentRow>().Select(r=>(r.X,r.Y)).ToHashSet();isolated.Selected=points;context.Selected=points;isolated.InvalidateVisual();context.InvalidateVisual();}
        fragments.SelectionChanged+=(_,_)=>UpdateFragments();isolated.FragmentClicked=context.FragmentClicked=point=>
        {var row=rows.FirstOrDefault(r=>r.X==point.X&&r.Y==point.Y);if(row!=null){if(fragments.SelectedItems!.Contains(row))fragments.SelectedItems.Remove(row);else fragments.SelectedItems.Add(row);}};
        var edits=new WrapPanel();details.Children.Add(edits);
        edits.Children.Add(Button("Split selected fragments",()=>Command(()=>Review!.RemoveFragments(c.Id,fragments.SelectedItems!.OfType<FragmentRow>().Select(r=>(r.X,r.Y)),true))));
        edits.Children.Add(Button("Exclude fragments",()=>Command(()=>Review!.RemoveFragments(c.Id,fragments.SelectedItems!.OfType<FragmentRow>().Select(r=>(r.X,r.Y)),false))));
        details.Children.Add(new TextBlock{Text=LocalText.T("Collision: pink = low, blue = high. Nearby flags start unchecked."),FontSize=11,TextWrapping=TextWrapping.Wrap});
        var collisions=new StackPanel();var evidence=c.SuggestedCollisions.Concat(c.Cells.Where(s=>(s.Cell.BackImage&ExtractionContent.CollisionMask)!=0)).GroupBy(s=>(s.X,s.Y));
        foreach(var group in evidence.OrderBy(g=>g.Key.Y).ThenBy(g=>g.Key.X))
        {
            int flags=group.Aggregate(0,(bits,s)=>bits|s.Cell.BackImage)&ExtractionContent.CollisionMask;
            int owned=c.Cells.Where(s=>s.X==group.Key.X&&s.Y==group.Key.Y).Aggregate(0,(bits,s)=>bits|s.Cell.BackImage)&ExtractionContent.CollisionMask;
            var row=new StackPanel{Orientation=Orientation.Horizontal,Spacing=6};row.Children.Add(new TextBlock{Text=$"({group.Key.X}, {group.Key.Y})",Width=90,VerticalAlignment=VerticalAlignment.Center});
            var low=new CheckBox{Content=LocalText.T("Low"),IsChecked=(owned&0x20000000)!=0,IsEnabled=(flags&0x20000000)!=0};
            var high=new CheckBox{Content=LocalText.T("High"),IsChecked=(owned&0x40000000)!=0,IsEnabled=(flags&0x40000000)!=0};row.Children.Add(low);row.Children.Add(high);collisions.Children.Add(row);
            void Toggle()=>Command(()=>Review!.Collision(c.Id,group.Key.X,group.Key.Y,(low.IsChecked==true?0x20000000:0)|(high.IsChecked==true?0x40000000:0)));
            low.IsCheckedChanged+=(_,_)=>Toggle();high.IsCheckedChanged+=(_,_)=>Toggle();
        }
        details.Children.Add(new ScrollViewer{Content=collisions,MaxHeight=170});
        details.Children.Add(new TextBlock{Text=LocalText.T("Sources"),FontWeight=FontWeight.SemiBold});
        details.Children.Add(new TextBlock{Text=string.Join("\n",c.Occurrences.Take(50).Select(o=>$"{o.Source}  ({o.X}, {o.Y})"+(o.CrossesSelection?"  "+LocalText.T("Selection boundary"):""))),TextWrapping=TextWrapping.Wrap,FontSize=11});
        ToolTip.SetTip(details,string.Join("\n",c.Occurrences.Select(o=>$"{o.Source} ({o.X}, {o.Y})")));
    }
    private async Task DetailPreview(ExtractionPreview target,ExtractionCandidate c,bool context)
    {
        using var lifetime=CancellationTokenSource.CreateLinkedTokenSource(previewStop.Token);
        void Detached(object? s,VisualTreeAttachmentEventArgs e)=>lifetime.Cancel();
        target.DetachedFromVisualTree+=Detached;
        var token=lifetime.Token;var store=assets!;bool entered=false;
        try
        {
            await previewSlots.WaitAsync(token);entered=true;var cells=context?c.Context:c.Cells;
            var result=await Task.Run(()=>
            {
                Rect area=default;foreach(var s in cells)
                {
                    token.ThrowIfCancellationRequested();area=EditorGeometry.Union(area,new Rect(s.X*48,s.Y*32,48,32));
                    if(s.Cell.Foreground is {} f&&store.Libraries.TryGetValue(f.Library,out var lib)&&lib.ReadHeader((f.Image&0x7FFF)-1) is {} h)
                        area=EditorGeometry.Union(area,MapSpriteDrawing.Bounds(h,s.X,s.Y,f.Library,(f.Image&0x7FFF)-1,true,true,MapSpriteDrawing.FrontBlend(f.Frames)));
                }
                if(context&&c.ContextArea is {} contextArea)area=contextArea;
                if(area.Width<=0||area.Height<=0)area=new(0,0,48,32);
                return (Bitmap:StampPreview.Create(cells,store,600,380,token,area,true,true),Area:area);
            },token);
            if(result.Bitmap==null)return;if(token.IsCancellationRequested||closed||target.GetVisualRoot()==null){result.Bitmap.Dispose();return;}
            HoldPreview(target,result.Bitmap);target.Image=result.Bitmap;target.Area=result.Area;target.Candidate=c;target.Context=context;target.InvalidateVisual();
        }
        catch(OperationCanceledException){}catch(Exception ex){StudioLog.Error("Extraction.DetailPreview",ex,$"Candidate={c.Id}; Name={c.Name}; Context={context}; Cells={c.Cells.Count}");if(!closed)ToolTip.SetTip(target,ex.Message);}
        finally{target.DetachedFromVisualTree-=Detached;if(entered)previewSlots.Release();}
    }
    private sealed record FragmentRow(int X,int Y,string Text);
}

internal sealed class ExtractionPreview:Control
{
    public RasterImage? Image;
    public Rect Area;
    public ExtractionCandidate? Candidate;
    public bool Context;
    public HashSet<(int X,int Y)> Selected=[];
    public Action<(int X,int Y)>? FragmentClicked;
    private (double Scale,double X,double Y) Transform()
    {
        if(Image==null)return(1,0,0);double fit=Math.Min(Bounds.Width/Image.PixelWidth,Bounds.Height/Image.PixelHeight);
        double scale=Math.Min((Image.PixelWidth-8)/Area.Width,(Image.PixelHeight-8)/Area.Height)*fit;
        return(scale,Bounds.Width/2-Area.Center.X*scale,Bounds.Height/2-Area.Center.Y*scale);
    }
    public override void Render(DrawingContext dc)
    {
        dc.FillRectangle(new SolidColorBrush(Color.FromRgb(24,28,34)),new Rect(Bounds.Size));if(Image==null||Candidate==null)return;
        double fit=Math.Min(Bounds.Width/Image.PixelWidth,Bounds.Height/Image.PixelHeight);var dest=new Rect((Bounds.Width-Image.PixelWidth*fit)/2,(Bounds.Height-Image.PixelHeight*fit)/2,Image.PixelWidth*fit,Image.PixelHeight*fit);
        dc.DrawImage(Image,dest);var t=Transform();
        foreach(var s in Context?Candidate.Context:Candidate.Cells)
        {
            var rect=new Rect(s.X*48*t.Scale+t.X,s.Y*32*t.Scale+t.Y,48*t.Scale,32*t.Scale);
            if((s.Cell.BackImage&0x20000000)!=0)dc.FillRectangle(new SolidColorBrush(Color.FromArgb(65,255,90,180)),rect);
            if((s.Cell.BackImage&0x40000000)!=0)dc.FillRectangle(new SolidColorBrush(Color.FromArgb(65,70,155,255)),rect);
        }
        foreach(var s in Candidate.Cells.Where(s=>s.Cell.HasForeground))
        {
            var rect=new Rect(s.X*48*t.Scale+t.X,s.Y*32*t.Scale+t.Y,48*t.Scale,32*t.Scale);
            dc.DrawRectangle(null,new Pen(Selected.Contains((s.X,s.Y))?Brushes.Gold:Brushes.LimeGreen,1),rect);
        }
    }
    protected override void OnPointerPressed(PointerPressedEventArgs e)
    {
        base.OnPointerPressed(e);if(Image==null)return;var t=Transform();var p=e.GetPosition(this);
        FragmentClicked?.Invoke(((int)Math.Floor((p.X-t.X)/t.Scale/48),(int)Math.Floor((p.Y-t.Y)/t.Scale/32)));
    }
}
