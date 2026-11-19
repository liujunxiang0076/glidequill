using System;
using System.Drawing;
using System.Windows.Forms;
using System.Collections.Generic;
using System.Drawing.Drawing2D;
using System.Runtime.InteropServices;

class RuleWindow : Form {
    int group;
    readonly string[] scopes = {"通用规则","浏览器"};
    readonly Button[] scopeButtons = {new Button(),new Button()};
    readonly Label actionSummary = new Label();
    readonly Label detailCaption = new Label();
    readonly Label listTitle = new Label();
    ListView list = new ListView();
    Panel pad = new Panel();
    Label heading = new Label(), result = new Label();
    Button edit = new Button(), pause = new Button();
    StatusStrip status = new StatusStrip();
    ToolStripStatusLabel state = new ToolStripStatusLabel();
    string[] gestures = {"向左划","向右划","向上划","向下划","先下后右"};
    string[] symbols = {"←","→","↑","↓","↳"};
    string[][] actions = {new[]{"后退","前进","复制","粘贴","关闭窗口"},new[]{"后退","前进","页面顶部","页面底部","关闭标签页"}};
    bool[][] enabled = {new[]{true,true,true,true,false},new[]{true,true,true,true,true}};
    bool paused, drawing;
    List<Point> points = new List<Point>();
    int Index {get {return list.SelectedIndices.Count==0 ? -1 : list.SelectedIndices[0];}}
    static void Place(Control parent, Control c, int x,int y,int w,int h,string text) {
        c.SetBounds(x,y,w,h); c.Text=text; parent.Controls.Add(c);
    }
    bool dark;
    Button theme = new Button();
    Color Tone(string light, string night) { return ColorTranslator.FromHtml(dark ? night : light); }
    Color Surface { get { return Tone("#ffffff","#181818"); } }
    Color Ink { get { return Tone("#232b32","#edf1f3"); } }
    Color Accent { get { return Tone("#176778","#83d5df"); } }
    Color Band { get { return Tone("#eef2f3","#272727"); } }
    [DllImport("dwmapi.dll")] static extern int DwmSetWindowAttribute(IntPtr hwnd,int attr,ref int val,int size);
    void StyleControls(Control parent) {
        parent.ForeColor=Ink; parent.BackColor=Surface;
        foreach(Control c in parent.Controls) {
            StyleControls(c);
            var b=c as Button;
            if(b!=null){ b.UseVisualStyleBackColor=false; b.FlatStyle=FlatStyle.Flat; b.FlatAppearance.BorderColor=Tone("#a6b2b7","#626d74");
                b.FlatAppearance.MouseOverBackColor=b==edit?Tone("#105969","#a4e4eb"):Tone("#e1ebec","#38474a");
                b.FlatAppearance.MouseDownBackColor=b==edit?Tone("#104653","#75b7bf"):Tone("#cbdfe1","#45585c");
                b.BackColor=b==edit?Accent:Band; b.ForeColor=b==edit?Tone("#ffffff","#142329"):Ink; }
            var box=c as ComboBox;
            if(box!=null){box.FlatStyle=FlatStyle.Flat;box.BackColor=Band;}
        }
    }
    void TitleTheme(Form f){int v=dark?1:0;DwmSetWindowAttribute(f.Handle,20,ref v,4);int caption=ColorTranslator.ToWin32(Band);DwmSetWindowAttribute(f.Handle,35,ref caption,4);int text=ColorTranslator.ToWin32(Ink);DwmSetWindowAttribute(f.Handle,36,ref text,4);}
    void ApplyTheme(){
        StyleControls(this);TitleTheme(this);theme.Text=dark?"外观：深色":"外观：浅色";
        pad.BackColor=Tone("#edf4f4","#1f1f1f");status.BackColor=Band;status.ForeColor=Ink;
        for(int n=0;n<scopeButtons.Length;n++){
            var tab=scopeButtons[n];
            tab.BackColor=n==group?Ink:Surface;
            tab.ForeColor=n==group?Surface:Ink;
            tab.FlatAppearance.BorderSize=n==group?0:1;
            tab.FlatAppearance.MouseOverBackColor=n==group?Tone("#344148","#d3e3e4"):Band;
            tab.FlatAppearance.MouseDownBackColor=tab.FlatAppearance.MouseOverBackColor;
            tab.AccessibleDescription=n==group?"当前范围":"切换到此范围";
        }
        actionSummary.ForeColor=Accent;
        detailCaption.ForeColor=Tone("#4c6063","#b5c4c6");
        list.Invalidate();pad.Invalidate();Refresh();
    }
    void DrawCell(Graphics g, Rectangle bounds,string text,Color bg,Color fg){
        using(var fill=new SolidBrush(bg))g.FillRectangle(fill,bounds);
        bounds.Inflate(-8,0);
        TextRenderer.DrawText(g,text,list.Font,bounds,fg,TextFormatFlags.VerticalCenter|TextFormatFlags.Left|TextFormatFlags.EndEllipsis);
    }
    public RuleWindow() {
        Text="GlideQuill  ·  手势规则"; StartPosition=FormStartPosition.CenterScreen;
        Font=new Font("Microsoft YaHei UI",9); ClientSize=new Size(920,496); MinimumSize=new Size(930,530);
        BackColor=SystemColors.Window; AutoScaleMode=AutoScaleMode.Dpi;
        var tools=new Panel();tools.Dock=DockStyle.Top;tools.Height=68;Controls.Add(tools);
        Place(tools,listTitle,24,16,210,40,"手势规则");listTitle.Font=new Font(Font.FontFamily,16,FontStyle.Bold);
        Place(tools,theme,624,16,128,36,"");theme.Anchor=AnchorStyles.Right|AnchorStyles.Top;
        theme.Click+=(s,e)=>{dark=!dark;ApplyTheme();};
        Place(tools,pause,764,16,132,36,"暂停手势");pause.Anchor=AnchorStyles.Right|AnchorStyles.Top;
        pause.Click+=(s,e)=>{paused=!paused;pause.Text=paused?"恢复手势":"暂停手势";state.Text=paused?"已暂停":"已启用";ResetPreview();};
        var split=new SplitContainer();split.Dock=DockStyle.Fill;split.FixedPanel=FixedPanel.Panel2;split.SplitterWidth=2;split.BackColor=SystemColors.ControlLight;Controls.Add(split);split.BringToFront();
        split.Panel1.BackColor=split.Panel2.BackColor=SystemColors.Window;split.SplitterDistance=534;
        list.Dock=DockStyle.Fill;list.View=View.Details;list.FullRowSelect=true;list.HideSelection=false;list.MultiSelect=false;list.BorderStyle=BorderStyle.None;
        list.OwnerDraw=true;
        list.DrawColumnHeader+=(s,e)=>DrawCell(e.Graphics,e.Bounds,e.Header.Text,Band,Ink);
        list.DrawSubItem+=(s,e)=>{bool selected=e.Item.Selected;
            DrawCell(e.Graphics,e.Bounds,e.SubItem.Text,selected?Tone("#d9ebed","#314b52"):Surface,
                e.ColumnIndex==0 && selected?Accent:Ink);};
        var heights=new ImageList();heights.ImageSize=new Size(1,46);list.SmallImageList=heights;Disposed+=(s,e)=>heights.Dispose();
        list.Columns.Add("手势",185);list.Columns.Add("执行动作",176);list.Columns.Add("状态",100);
        split.Panel1.Padding=new Padding(24,8,16,16);split.Panel1.Controls.Add(list);list.Resize+=(s,e)=>list.Columns[2].Width=Math.Max(90,list.ClientSize.Width-list.Columns[0].Width-list.Columns[1].Width);
        var scopeBar=new Panel();scopeBar.Height=56;scopeBar.Dock=DockStyle.Top;
        split.Panel1.Controls.Add(scopeBar);list.BringToFront();
        for(int n=0;n<scopes.Length;n++){
            int target=n;
            Place(scopeBar,scopeButtons[n],n*120,0,112,34,scopes[n]);
            scopeButtons[n].Click+=(s,e)=>{group=target;Reload(0);ApplyTheme();};
        }
        var side=split.Panel2;
        Place(side,detailCaption,24,12,300,24,"");Place(side,heading,24,44,300,40,"");
        heading.Font=new Font(Font.FontFamily,16,FontStyle.Bold);
        Place(side,actionSummary,24,88,310,28,"");
        Place(side,pad,24,128,320,136,"");pad.BorderStyle=BorderStyle.None;
        Place(side,result,24,276,326,40,"按住右键，在框内试画");
        Place(side,edit,24,336,144,40,"编辑规则…");
        edit.Click+=(s,e)=>EditRule(); list.DoubleClick+=(s,e)=>EditRule();
        list.KeyDown+=(s,e)=>{if(e.KeyCode==Keys.Enter)EditRule();};
        list.SelectedIndexChanged+=(s,e)=>ResetPreview();
        pad.Paint+=PaintPad;pad.MouseDown+=(s,e)=>{if(paused||e.Button!=MouseButtons.Right)return;drawing=true;points.Clear();points.Add(e.Location);pad.Capture=true;pad.Invalidate();};
        pad.MouseMove+=(s,e)=>{if(drawing){points.Add(e.Location);pad.Invalidate();}};
        pad.MouseUp+=(s,e)=>{if(!drawing)return;drawing=false;pad.Capture=false;points.Add(e.Location);int dx=e.X-points[0].X,dy=e.Y-points[0].Y;
            int down=0;foreach(Point q in points)down=Math.Max(down,q.Y-points[0].Y);
            int n=down>25 && dx>25 && points.Exists(q=>q.Y-points[0].Y>20 && Math.Abs(q.X-points[0].X)<15)?4:Math.Abs(dx)>Math.Abs(dy)?(dx<0?0:1):(dy<0?2:3);
            result.Text=Math.Max(Math.Abs(dx),Math.Abs(dy))<15?"轨迹太短，请再试一次":gestures[n]+" → "+(enabled[group][n]?actions[group][n]:"已停用");pad.Invalidate();};
        status.Items.Add(state);state.Text="已启用";status.Items.Add(new ToolStripStatusLabel(){Spring=true});status.Items.Add("本次预览有效");Controls.Add(status);
        group=0;Reload(0);ApplyTheme();
    }
    void Reload(int index){list.Items.Clear();int g=group;for(int i=0;i<gestures.Length;i++){var item=new ListViewItem(symbols[i]+"     "+gestures[i]);item.SubItems.Add(actions[g][i]);item.SubItems.Add(enabled[g][i]?"启用":"停用");list.Items.Add(item);}list.Items[index].Selected=true;list.Items[index].Focused=true;ResetPreview();}
    void ResetPreview(){drawing=false;pad.Capture=false;points.Clear();int i=Index;detailCaption.Text=scopes[group]+"  /  规则详情";actionSummary.Text=i<0?"":"执行动作    "+actions[group][i];heading.Text=i<0?"选择一条规则":gestures[i];edit.Enabled=i>=0;result.Text=paused?"手势已暂停":"按住右键，在框内试画";pad.Invalidate();}
    void PaintPad(object sender,PaintEventArgs e){int i=Index;if(i<0)return;Point[][] paths={new[]{new Point(215,68),new Point(62,68)},new[]{new Point(62,68),new Point(215,68)},new[]{new Point(138,106),new Point(138,29)},new[]{new Point(138,29),new Point(138,106)},new[]{new Point(85,33),new Point(85,99),new Point(203,99)}};
        Point[] path=points.Count>1?points.ToArray():paths[i];e.Graphics.SmoothingMode=SmoothingMode.AntiAlias;using(var pen=new Pen(Accent,3)){if(points.Count==0)pen.EndCap=LineCap.ArrowAnchor;e.Graphics.DrawLines(pen,path);var p=path[0];e.Graphics.FillEllipse(Brushes.White,p.X-4,p.Y-4,8,8);e.Graphics.DrawEllipse(pen,p.X-4,p.Y-4,8,8);}}
    void EditRule(){int i=Index;if(i<0)return;int g=group;using(var f=new Form()){f.Text="编辑规则";f.Font=Font;f.StartPosition=FormStartPosition.CenterParent;f.FormBorderStyle=FormBorderStyle.FixedDialog;f.MaximizeBox=f.MinimizeBox=false;f.ClientSize=new Size(346,187);
        Place(f,new Label(),16,15,305,25,scopes[group]+"  /  "+gestures[i]);Place(f,new Label(),16,59,75,26,"执行动作");var a=new ComboBox();a.DropDownStyle=ComboBoxStyle.DropDownList;Place(f,a,92,54,234,28,"");a.Items.AddRange(new[]{"后退","前进","复制","粘贴","关闭窗口","页面顶部","页面底部","关闭标签页"});a.SelectedItem=actions[g][i];
        var check=new CheckBox();Place(f,check,92,91,195,26,"启用这条规则");check.Checked=enabled[g][i];
        var ok=new Button();var cancel=new Button();Place(f,ok,154,139,80,31,"确定");Place(f,cancel,246,139,80,31,"取消");ok.DialogResult=DialogResult.OK;cancel.DialogResult=DialogResult.Cancel;f.AcceptButton=ok;f.CancelButton=cancel;
        StyleControls(f);TitleTheme(f);
        if(f.ShowDialog(this)==DialogResult.OK){actions[g][i]=(string)a.SelectedItem;enabled[g][i]=check.Checked;Reload(i);}}}
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr hwnd,IntPtr dc,uint flags);
    [DllImport("user32.dll")] static extern bool SetProcessDPIAware();
    [STAThread] static void Main(string[] args){SetProcessDPIAware();Application.EnableVisualStyles();Application.SetCompatibleTextRenderingDefault(false);
        using(var w=new RuleWindow()){if(args.Length>=2 && args[0]=="--preview"){w.Shown+=(s,e)=>w.BeginInvoke(new Action(()=>{if(args.Length==3 && args[2]=="dark")w.theme.PerformClick();w.Refresh();using(var b=new Bitmap(w.Width,w.Height)){using(var g=Graphics.FromImage(b)){var dc=g.GetHdc();try{PrintWindow(w.Handle,dc,2);}finally{g.ReleaseHdc(dc);}}b.Save(args[1],System.Drawing.Imaging.ImageFormat.Png);}w.Close();}));}Application.Run(w);}}
}
