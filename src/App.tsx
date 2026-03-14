import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import Markdown from "react-markdown";
import "./App.css";

interface FeedItem {
  title: string;
  link: string;
  pub_date: string | null;
  description: string | null;
  content: string | null;
}

interface Feed {
  title: string;
  link: string;
  description: string;
  items: FeedItem[];
}

interface Comment {
  author: string | null;
  date: string | null;
  text: string;
}

interface ExtractedContent {
  title: string;
  text: string;
  images: string[];
  videos: { url: string; thumbnail: string | null; platform: string | null }[];
  byline: string | null;
  comments: Comment[];
}

interface SavedFeed {
  url: string;
  name: string;
  confirmDelete?: boolean;
}

function getYoutubeId(url: string): string {
  const urlLower = url.toLowerCase();
  
  if (urlLower.includes('youtube.com/embed/') || urlLower.includes('youtube-nocookie.com/embed/')) {
    const match = url.match(/embed\/([^/?]+)/);
    return match ? match[1] : '';
  }
  
  if (urlLower.includes('youtu.be/')) {
    const match = url.match(/youtu\.be\/([^?]+)/);
    return match ? match[1] : '';
  }
  
  if (urlLower.includes('youtube.com/watch')) {
    const match = url.match(/v=([^&]+)/);
    return match ? match[1] : '';
  }
  
  return '';
}

function getYoutubeEmbedUrl(url: string): string {
  const urlLower = url.toLowerCase();
  
  if (urlLower.includes('youtube.com/embed/') || urlLower.includes('youtube-nocookie.com/embed/')) {
    return url;
  }
  
  const videoId = getYoutubeId(url);
  if (videoId) {
    return `https://www.youtube.com/embed/${videoId}`;
  }
  
  return '';
}

function App() {
  const [feeds, setFeeds] = useState<SavedFeed[]>([]);
  const [newFeedUrl, setNewFeedUrl] = useState("");
  const [selectedFeed, setSelectedFeed] = useState<SavedFeed | null>(null);
  const [feedData, setFeedData] = useState<Feed | null>(null);
  const [selectedArticle, setSelectedArticle] = useState<FeedItem | null>(null);
  const [extractedContent, setExtractedContent] = useState<ExtractedContent | null>(null);
  const [loading, setLoading] = useState(false);
  const [articleLoading, setArticleLoading] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [articlesCollapsed, setArticlesCollapsed] = useState(false);
  const [filterMode, setFilterMode] = useState(false);
  const [theme, setTheme] = useState<'light' | 'dark' | 'system'>(() => {
    const saved = localStorage.getItem('rss-theme');
    return (saved as 'light' | 'dark' | 'system') || 'system';
  });
  const [filters, setFilters] = useState<string[]>(() => {
    const saved = localStorage.getItem('rss-filters');
    return saved ? JSON.parse(saved) : [];
  });
  const [readArticles, setReadArticles] = useState<Set<string>>(() => {
    const saved = localStorage.getItem('rss-read-articles');
    return saved ? new Set(JSON.parse(saved)) : new Set();
  });
  const [isMarkdown, setIsMarkdown] = useState(false);

  const markAsRead = (url: string) => {
    const newRead = new Set(readArticles).add(url);
    setReadArticles(newRead);
    localStorage.setItem('rss-read-articles', JSON.stringify([...newRead]));
  };

  const markAllAsRead = () => {
    if (!feedData) return;
    const allUrls = feedData.items.map(item => item.link);
    const newRead = new Set([...readArticles, ...allUrls]);
    setReadArticles(newRead);
    localStorage.setItem('rss-read-articles', JSON.stringify([...newRead]));
  };

  const isRead = (url: string) => readArticles.has(url);

  const saveFilters = (newFilters: string[]) => {
    setFilters(newFilters);
    localStorage.setItem('rss-filters', JSON.stringify(newFilters));
  };

  const addFilter = async (text: string) => {
    if (!text || text.length < 3) return;
    const snippet = text.substring(0, 50).toLowerCase();
    if (!filters.includes(snippet)) {
      const newFilters = [...filters, snippet];
      saveFilters(newFilters);
      
      if (selectedArticle) {
        setArticleLoading(true);
        try {
          const content = await invoke<ExtractedContent>("extract_content", { 
            url: selectedArticle.link,
            filters: newFilters
          });
          setExtractedContent(content);
        } catch (e) {
          console.error("Failed to extract content:", e);
        } finally {
          setArticleLoading(false);
        }
      }
    }
    setFilterMode(false);
  };

  const removeFilter = async (filter: string) => {
    const newFilters = filters.filter(f => f !== filter);
    saveFilters(newFilters);
    
    if (selectedArticle) {
      setArticleLoading(true);
      try {
        const content = await invoke<ExtractedContent>("extract_content", { 
          url: selectedArticle.link,
          filters: newFilters
        });
        setExtractedContent(content);
      } catch (e) {
        console.error("Failed to extract content:", e);
      } finally {
        setArticleLoading(false);
      }
    }
  };

  useEffect(() => {
    const saved = localStorage.getItem("rss-feeds");
    if (saved) {
      setFeeds(JSON.parse(saved));
    }
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    if (theme === 'dark') {
      root.classList.add('dark');
    } else if (theme === 'light') {
      root.classList.remove('dark');
    } else {
      root.classList.remove('dark');
    }
    localStorage.setItem('rss-theme', theme);
  }, [theme]);

  const toggleTheme = () => {
    setTheme(prev => prev === 'dark' ? 'light' : 'dark');
  };

  const saveFeeds = (newFeeds: SavedFeed[]) => {
    setFeeds(newFeeds);
    localStorage.setItem("rss-feeds", JSON.stringify(newFeeds));
  };

  const addFeed = async () => {
    if (!newFeedUrl.trim()) return;
    
    try {
      setLoading(true);
      const feed = await invoke<Feed>("fetch_feed", { url: newFeedUrl });
      const newFeed: SavedFeed = { url: newFeedUrl, name: feed.title };
      saveFeeds([...feeds, newFeed]);
      setNewFeedUrl("");
    } catch (e) {
      alert("Failed to fetch feed: " + e);
    } finally {
      setLoading(false);
    }
  };

  const removeFeed = (url: string) => {
    const feed = feeds.find(f => f.url === url);
    if (!feed) return;
    
    if (feed.confirmDelete) {
      saveFeeds(feeds.filter(f => f.url !== url));
      if (selectedFeed?.url === url) {
        setSelectedFeed(null);
        setFeedData(null);
        setSelectedArticle(null);
      }
    } else {
      saveFeeds(feeds.map(f => f.url === url ? { ...f, confirmDelete: true } : f));
    }
  };
  
  const cancelDelete = (url: string) => {
    saveFeeds(feeds.map(f => f.url === url ? { ...f, confirmDelete: false } : f));
  };

  const selectFeed = async (feed: SavedFeed) => {
    setSelectedFeed(feed);
    setSelectedArticle(null);
    setExtractedContent(null);
    
    try {
      setLoading(true);
      const data = await invoke<Feed>("fetch_feed", { url: feed.url });
      setFeedData(data);
    } catch (e) {
      alert("Failed to fetch feed: " + e);
    } finally {
      setLoading(false);
    }
  };

  const selectArticle = async (item: FeedItem) => {
    markAsRead(item.link);
    setSelectedArticle(item);
    setExtractedContent(null);
    
    const isGitHub = item.link.includes('github.com') || item.link.includes('github.io');
    setIsMarkdown(isGitHub);
    
    try {
      setArticleLoading(true);
      const content = await invoke<ExtractedContent>("extract_content", { 
        url: item.link,
        filters: filters
      });
      setExtractedContent(content);
    } catch (e) {
      console.error("Failed to extract content:", e);
    } finally {
      setArticleLoading(false);
    }
  };

  return (
    <div className="app">
      <div className={`sidebar ${sidebarCollapsed ? "collapsed" : ""}`}>
        <div className="sidebar-header">
          <button 
            className="collapse-btn" 
            onClick={() => setSidebarCollapsed(!sidebarCollapsed)}
            title={sidebarCollapsed ? "Expand feeds" : "Collapse feeds"}
          >
            {sidebarCollapsed ? "▶" : "◀"}
          </button>
          {!sidebarCollapsed && <h2>Feeds</h2>}
          <button 
            className="theme-toggle" 
            onClick={toggleTheme}
            title={theme === 'dark' ? "Switch to light mode" : "Switch to dark mode"}
          >
            {theme === 'dark' ? '☀' : '☾'}
          </button>
        </div>
        {!sidebarCollapsed && (
          <>
            <div className="add-feed">
              <input
                type="text"
                value={newFeedUrl}
                onChange={(e) => setNewFeedUrl(e.target.value)}
                placeholder="Enter RSS/Atom URL..."
                onKeyDown={(e) => e.key === "Enter" && addFeed()}
              />
              <button onClick={addFeed} disabled={loading}>
                {loading ? "..." : "+"}
              </button>
            </div>
            <div className="feed-list">
              {feeds.map((feed) => (
                <div
                  key={feed.url}
                  className={`feed-item ${selectedFeed?.url === feed.url ? "selected" : ""}`}
                  onClick={() => selectFeed(feed)}
                >
                  <span className="feed-name">{feed.name}</span>
                  {feed.confirmDelete ? (
                    <div className="delete-confirm">
                      <button
                        className="confirm-yes"
                        onClick={(e) => { e.stopPropagation(); removeFeed(feed.url); }}
                        title="Confirm delete"
                      >
                        ✓
                      </button>
                      <button
                        className="confirm-no"
                        onClick={(e) => { e.stopPropagation(); cancelDelete(feed.url); }}
                        title="Cancel"
                      >
                        ×
                      </button>
                    </div>
                  ) : (
                    <button
                      className="remove-feed"
                      onClick={(e) => { e.stopPropagation(); removeFeed(feed.url); }}
                    >
                      ×
                    </button>
                  )}
                </div>
              ))}
            </div>
          </>
        )}
      </div>
      
      <div className={`article-list ${articlesCollapsed ? "collapsed" : ""}`}>
        <div className="article-list-header">
          <button 
            className="collapse-btn" 
            onClick={() => setArticlesCollapsed(!articlesCollapsed)}
            title={articlesCollapsed ? "Expand articles" : "Collapse articles"}
          >
            {articlesCollapsed ? "▶" : "◀"}
          </button>
          <h2>{feedData?.title || "Select a feed"}</h2>
          {feedData && feedData.items.some(item => !isRead(item.link)) && (
            <button className="mark-all-read" onClick={markAllAsRead} title="Mark all as read">
              ✓
            </button>
          )}
        </div>
        {!articlesCollapsed && (
          <div className="articles">
            {feedData?.items.map((item, idx) => (
              <div
                key={idx}
                className={`article-item ${selectedArticle?.link === item.link ? "selected" : ""} ${isRead(item.link) ? "read" : ""}`}
                onClick={() => selectArticle(item)}
              >
                <div className="article-title">{item.title}</div>
                {item.pub_date && (
                  <div className="article-date">
                    {new Date(item.pub_date).toLocaleDateString()}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
      
      <div className="reader">
        {articleLoading ? (
          <div className="loading">Loading article...</div>
        ) : extractedContent ? (
          <div className="content">
            <div className="content-header">
              <h1>{extractedContent.title}</h1>
                <div className="filter-controls">
                {selectedArticle && (
                  <a 
                    href={selectedArticle.link} 
                    target="_blank" 
                    rel="noopener noreferrer"
                    className="open-original"
                  >
                    ↗ Original
                  </a>
                )}
                <button 
                  className={`filter-toggle ${filterMode ? 'active' : ''}`}
                  onClick={() => setFilterMode(!filterMode)}
                  title={filterMode ? "Exit filter mode" : "Enter filter mode - click elements to filter"}
                >
                  {filterMode ? 'Filter' : 'Filter'}
                </button>
                {filters.length > 0 && (
                  <div className="filter-list">
                    {filters.map((f, i) => (
                      <span key={i} className="filter-tag" onClick={() => removeFilter(f)}>
                        {f.substring(0, 20)}... ×
                      </span>
                    ))}
                  </div>
                )}
              </div>
            </div>
            {extractedContent.byline && (
              <div className="byline">{extractedContent.byline}</div>
            )}
            <div className="article-content-split">
              <div className="article-text">
                {isMarkdown ? (
                  <Markdown>{extractedContent.text}</Markdown>
                ) : (
                  extractedContent.text.split('\n\n').map((para, i) => (
                    <p 
                      key={i} 
                      className={filterMode ? 'filter-clickable' : ''}
                      onClick={() => filterMode && addFilter(para)}
                      title={filterMode ? 'Click to filter this content' : ''}
                    >
                      {para}
                    </p>
                  ))
                )}
              </div>
              <div className="article-media">
                {extractedContent.images.length > 0 && (
                  <div className="article-images">
                    {extractedContent.images.slice(0, 10).map((img, i) => (
                      <img key={i} src={img} alt={`Image ${i + 1}`} />
                    ))}
                  </div>
                )}
                {extractedContent.videos.length > 0 && (
                  <div className="article-videos">
                    {extractedContent.videos.map((video, i) => {
                      const embedUrl = video.platform === 'YouTube' ? getYoutubeEmbedUrl(video.url) : '';
                      return (
                        <div key={i} className="video-container">
                          {embedUrl ? (
                            <iframe
                              src={embedUrl}
                              title={`Video ${i + 1}`}
                              frameBorder="0"
                              allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture"
                              allowFullScreen
                            />
                          ) : (
                            <video controls>
                              <source src={video.url} />
                              Your browser does not support the video tag.
                            </video>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </div>
            {extractedContent.comments.length > 0 && (
              <div className="comments-section">
                <h3>Comments ({extractedContent.comments.length})</h3>
                {extractedContent.comments.map((comment, i) => (
                  <div key={i} className="comment">
                    {comment.author && (
                      <div className="comment-author">{comment.author}</div>
                    )}
                    {comment.date && (
                      <div className="comment-date">{comment.date}</div>
                    )}
                    <div className="comment-text">
                      {comment.text.split('\n\n').map((para, j) => (
                        <p key={j}>{para}</p>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : selectedArticle ? (
          <div className="content">
            <h1>{selectedArticle.title}</h1>
            <a href={selectedArticle.link} target="_blank" rel="noopener noreferrer">
              Open original
            </a>
          </div>
        ) : (
          <div className="empty">Select an article to read</div>
        )}
      </div>
    </div>
  );
}

export default App;