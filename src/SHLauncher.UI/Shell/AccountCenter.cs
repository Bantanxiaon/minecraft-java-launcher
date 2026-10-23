using System.ComponentModel;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Layout;
using Avalonia.Media;
using SHLauncher.UI.ViewModels;

namespace SHLauncher.UI.Shell;

/// <summary>Anchored account actions, backed by the existing account coordinator.</summary>
public sealed class AccountCenter : UserControl, IAsyncDisposable
{
    private readonly StartupViewModel _startup;
    private readonly Button _button;
    private readonly SkinAvatar _avatar=new();
    private readonly SHLauncher.Contracts.Accounts.IProfileTextureService? _textures;
    private readonly SHLNWindow _window;
    private readonly CancellationTokenSource _lifetime=new();
    private Guid? _profile;
    private long _revision;
    private SHLauncher.Contracts.Accounts.ProfileTextures? _current;
    private Task _loading=Task.CompletedTask;
    private readonly List<Task> _pendingLoads=[];
    private Task LoadTracked(bool force){_pendingLoads.RemoveAll(t=>t.IsCompleted);var task=Load(force);_pendingLoads.Add(task);return task;}
    private readonly TextBlock _name = new() { MaxWidth=160, TextTrimming=TextTrimming.CharacterEllipsis };
    private readonly Flyout _flyout;
    private readonly StackPanel _menu = new() { Spacing=4, MinWidth=220 };
    public AccountCenter(StartupViewModel startup,SHLauncher.Contracts.Accounts.IProfileTextureService? textures,SHLNWindow window)
    {
        _startup=startup;_textures=textures;_window=window;
        _flyout=new Flyout { Content=_menu, Placement=PlacementMode.BottomEdgeAlignedRight };
        _button=new Button { Background=Brushes.Transparent, BorderThickness=new Thickness(0), Padding=new Thickness(10,5), Flyout=_flyout,
            Content=new StackPanel { Orientation=Orientation.Horizontal, Spacing=8, Children={_avatar,_name,new TextBlock {Text="⌄"}} } };
        Avalonia.Automation.AutomationProperties.SetName(_button,"账户中心");
        Content=_button;
        startup.Accounts.PropertyChanged+=Changed;
        _flyout.Opening+=Opening;
        Refresh();
    }
    private void Changed(object? sender,PropertyChangedEventArgs e)
    { if(e.PropertyName is nameof(AccountsViewModel.ActiveName) or nameof(AccountsViewModel.HasActive) or nameof(AccountsViewModel.Busy))Refresh(); }
    private void Refresh()
    {
        _name.Text=_startup.Accounts.ActiveName;_button.IsEnabled=_startup.Accounts.HasActive&&!_startup.Accounts.Busy;
        var account=_startup.Accounts.ActiveAccount;
        if(_profile!=account?.AccountId){_profile=account?.AccountId;_current=null;_avatar.SetSkin(null);_loading=LoadTracked(false);}
    }
    private async Task Load(bool force)
    {
        var revision=++_revision;var account=_startup.Accounts.ActiveAccount;
        if(account is null||_textures is null)return;
        try
        {
            var authority=_startup.Accounts.Authorities.FirstOrDefault(a=>a.ProviderId==account.ProviderId);
            var textures=await _textures.ReadAsync(account,authority,force,_lifetime.Token);
            if(_lifetime.IsCancellationRequested||revision!=_revision)return;
            _current=textures;_avatar.SetSkin(textures.Skin);
        }
        catch(OperationCanceledException) { }
        catch { if(!_lifetime.IsCancellationRequested&&revision==_revision)_window.ShowToast("纹理暂不可用，使用默认皮肤"); }
    }
    private async void ShowTextures()
    {
        await _loading;if(_lifetime.IsCancellationRequested)return;
        using var preview=new SkinAvatar {Width=96,Height=96};preview.SetSkin(_current?.Skin);
        using var cape=new CapePreview();cape.SetTexture(_current?.Cape);
        var status=new TextBlock {Text=_current?.Status??"使用 SHLN 默认皮肤",TextWrapping=TextWrapping.Wrap,MaxWidth=430};
        var capeStatus=new TextBlock {Text=_current?.Cape is null?"此角色暂无可用披风":"当前披风",Opacity=.7};
        var refresh=new Button {Content="刷新纹理",IsVisible=_current?.Capabilities.CanReadSkin==true};
        var closed=false;Task refreshing=Task.CompletedTask;
        async Task RefreshTextures()
        {
            refresh.IsEnabled=false;
            try{await (_loading=LoadTracked(true));if(closed||_lifetime.IsCancellationRequested)return;preview.SetSkin(_current?.Skin);cape.SetTexture(_current?.Cape);status.Text=_current?.Status;capeStatus.Text=_current?.Cape is null?"此角色暂无可用披风":"当前披风";}
            finally{if(!closed)refresh.IsEnabled=true;}
        }
        refresh.Click+=(_,_)=>refreshing=RefreshTextures();
        var visuals=new StackPanel {Orientation=Orientation.Horizontal,Spacing=28,Children={new StackPanel{Spacing=8,Children={preview,new TextBlock{Text="皮肤头像"}}},new StackPanel{Spacing=8,Children={cape,capeStatus}}}};
        var content=new StackPanel {Spacing=16,Children={visuals,status,new TextBlock {Text="当前认证服务未提供上传皮肤或披风的授权。",FontSize=12,Opacity=.65},refresh}};
        try{await _window.ShowSheetAsync("皮肤与披风",content);}finally{closed=true;await refreshing;}
    }
    private void SwitchAccount()
    {
        _menu.Children.Clear();
        _menu.Children.Add(new TextBlock{Text="切换账户",Margin=new Thickness(8),FontWeight=FontWeight.SemiBold});
        foreach(var account in _startup.Accounts.Accounts)
        {
            var selected=account;
            Add(selected.ProfileName,()=>{_startup.Accounts.SelectedAccount=selected;_startup.Accounts.ActivateCommand.Execute(null);});
        }
        Add("添加账户",Manage);
    }
    private void Opening(object? sender,EventArgs e)
    {
        var accounts=_startup.Accounts;
        _menu.Children.Clear();
        _menu.Children.Add(new TextBlock {Text=accounts.ActiveName,FontWeight=FontWeight.SemiBold,Margin=new Thickness(8,4)});
        _menu.Children.Add(new TextBlock {Text=accounts.ActiveProvider,Opacity=.65,FontSize=12,Margin=new Thickness(8,0,8,8)});
        var switchButton=new Button{Content="切换账户",HorizontalAlignment=HorizontalAlignment.Stretch,HorizontalContentAlignment=HorizontalAlignment.Left};switchButton.Click+=(_,_)=>SwitchAccount();_menu.Children.Add(switchButton);Add("添加账户",Manage);Add("账号管理",Manage);Add("皮肤与披风",ShowTextures);
        Add("退出当前账户",async()=>
        {
            accounts.SelectedAccount=accounts.ActiveAccount;
            accounts.LogoutCommand.Execute(null);
            await accounts.LogoutCommand.DrainAsync();
            if(!accounts.HasActive) _startup.ManageAccountsCommand.Execute(null);
        });
    }
    private void Manage() { _startup.Accounts.BackCommand.Execute(null);_startup.ManageAccountsCommand.Execute(null); }
    private void Add(string text,Action action)
    {
        var button=new Button {Content=text,HorizontalAlignment=HorizontalAlignment.Stretch,HorizontalContentAlignment=HorizontalAlignment.Left};
        button.Click+=(_,_)=>{_flyout.Hide();action();};_menu.Children.Add(button);
    }
    public async ValueTask DisposeAsync() { _lifetime.Cancel();await Task.WhenAll(_pendingLoads);_avatar.Dispose();_flyout.Hide();_flyout.Opening-=Opening;_startup.Accounts.PropertyChanged-=Changed;_lifetime.Dispose(); }
}
