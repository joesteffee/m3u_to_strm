#!/usr/bin/env python3
"""
Comprehensive test suite for parse_m3u.py
"""
import os
import pytest
import tempfile
import shutil
from pathlib import Path
from unittest.mock import patch, MagicMock, Mock
import requests_mock

# Import the module functions
import sys
sys.path.insert(0, str(Path(__file__).parent))

# We'll need to import functions after setting up the environment
# For now, let's test the logic by importing and patching


class TestFilenameParsing:
    """Test filename parsing and sanitization functions"""
    
    def test_safe_filename(self):
        """Test safe_filename removes invalid characters"""
        from parse_m3u import safe_filename
        
        assert safe_filename("Movie: Name") == "Movie Name"
        assert safe_filename("Movie/Name") == "MovieName"
        assert safe_filename("Movie*Name") == "MovieName"
        assert safe_filename("Movie?Name") == "MovieName"
        assert safe_filename("Movie<Name>") == "MovieName"
        assert safe_filename("Movie|Name") == "MovieName"
        assert safe_filename("  Movie Name  ") == "Movie Name"
        assert safe_filename("Movie\\Name") == "MovieName"
    
    def test_parse_movie_name(self):
        """Test movie name parsing"""
        from parse_m3u import parse_movie_name
        
        # Remove language prefix
        assert parse_movie_name("EN - Movie Name") == "Movie Name"
        assert parse_movie_name("FR - Movie Name") == "Movie Name"
        assert parse_movie_name("ES - Movie Name") == "Movie Name"
        
        # Preserve year at end
        assert parse_movie_name("EN - Movie Name (2023)") == "Movie Name (2023)"
        assert parse_movie_name("Movie Name (2023)") == "Movie Name (2023)"
        
        # Remove other parentheses content but keep year
        assert parse_movie_name("EN - Movie Name (Action) (2023)") == "Movie Name (2023)"
        
        # No year
        assert parse_movie_name("EN - Movie Name") == "Movie Name"
        
        # Invalid characters
        assert parse_movie_name("EN - Movie: Name (2023)") == "Movie Name (2023)"
    
    def test_parse_series_name(self):
        """Test series name parsing"""
        from parse_m3u import parse_series_name
        
        # Remove language prefix
        assert parse_series_name("EN - Series Name") == "Series Name"
        
        # Remove season/episode
        assert parse_series_name("EN - Series Name S01E01") == "Series Name"
        assert parse_series_name("EN - Series Name S2E5") == "Series Name"
        assert parse_series_name("EN - Series Name S12E15") == "Series Name"
        
        # Preserve year
        assert parse_series_name("EN - Series Name (2023) S01E01") == "Series Name (2023)"
        
        # Remove other parentheses but keep year
        assert parse_series_name("EN - Series Name (Drama) (2023) S01E01") == "Series Name (2023)"
    
    def test_extract_season_episode(self):
        """Test season and episode extraction"""
        from parse_m3u import extract_season_episode
        
        # Standard format
        season, episode = extract_season_episode("Series Name S01E01")
        assert season == "Season 1"
        assert episode == "S01E01"
        
        season, episode = extract_season_episode("Series Name S2E5")
        assert season == "Season 2"
        assert episode == "S02E05"
        
        season, episode = extract_season_episode("Series Name S12E15")
        assert season == "Season 12"
        assert episode == "S12E15"
        
        # Case insensitive
        season, episode = extract_season_episode("Series Name s01e01")
        assert season == "Season 1"
        assert episode == "S01E01"
        
        # No season/episode found
        season, episode = extract_season_episode("Series Name")
        assert season == "Season 1"
        assert episode == "S01E01"


class TestFileOperations:
    """Test STRM file writing operations"""
    
    def setup_method(self):
        """Set up temporary directory for each test"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.test_dir = self.temp_dir / "test"
    
    def teardown_method(self):
        """Clean up temporary directory"""
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_write_strm_file_new(self):
        """Test writing a new STRM file"""
        from parse_m3u import write_strm_file
        
        filepath, is_new, url_changed = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream.m3u8"
        )
        
        assert is_new is True
        assert url_changed is False
        assert filepath.exists()
        assert filepath.read_text().strip() == "http://example.com/stream.m3u8"
    
    def test_write_strm_file_existing_same_url(self):
        """Test writing to existing file with same URL"""
        from parse_m3u import write_strm_file
        
        # Create file first
        filepath, _, _ = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream.m3u8"
        )
        
        # Write again with same URL
        filepath2, is_new, url_changed = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream.m3u8"
        )
        
        assert filepath == filepath2
        assert is_new is False
        assert url_changed is False
    
    def test_write_strm_file_existing_different_url(self):
        """Test writing to existing file with different URL"""
        from parse_m3u import write_strm_file
        
        # Create file first
        filepath, _, _ = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream1.m3u8"
        )
        
        # Write again with different URL
        filepath2, is_new, url_changed = write_strm_file(
            self.test_dir, "test_movie", "http://example.com/stream2.m3u8"
        )
        
        assert filepath == filepath2
        assert is_new is False
        assert url_changed is True
        assert filepath2.read_text().strip() == "http://example.com/stream2.m3u8"


class TestPathConversion:
    """Test path conversion for Emby"""
    
    def setup_method(self):
        """Set up environment variables"""
        self.original_movies = os.environ.get("EMBY_MOVIES_PATH")
        self.original_series = os.environ.get("EMBY_SERIES_PATH")
        self.original_livetv = os.environ.get("EMBY_LIVETV_PATH")
    
    def teardown_method(self):
        """Restore environment variables"""
        if self.original_movies:
            os.environ["EMBY_MOVIES_PATH"] = self.original_movies
        elif "EMBY_MOVIES_PATH" in os.environ:
            del os.environ["EMBY_MOVIES_PATH"]
        
        if self.original_series:
            os.environ["EMBY_SERIES_PATH"] = self.original_series
        elif "EMBY_SERIES_PATH" in os.environ:
            del os.environ["EMBY_SERIES_PATH"]
        
        if self.original_livetv:
            os.environ["EMBY_LIVETV_PATH"] = self.original_livetv
        elif "EMBY_LIVETV_PATH" in os.environ:
            del os.environ["EMBY_LIVETV_PATH"]
        
        # Reload module to pick up new env vars
        if 'parse_m3u' in sys.modules:
            import importlib
            importlib.reload(sys.modules['parse_m3u'])
    
    def test_convert_to_emby_path_no_mapping(self):
        """Test path conversion when no mapping is set"""
        # Clear any existing mappings
        if "EMBY_MOVIES_PATH" in os.environ:
            del os.environ["EMBY_MOVIES_PATH"]
        if "EMBY_SERIES_PATH" in os.environ:
            del os.environ["EMBY_SERIES_PATH"]
        if "EMBY_LIVETV_PATH" in os.environ:
            del os.environ["EMBY_LIVETV_PATH"]
        
        # Reload module
        if 'parse_m3u' in sys.modules:
            import importlib
            importlib.reload(sys.modules['parse_m3u'])
        
        from parse_m3u import convert_to_emby_path, MOVIES_DIR
        
        path = MOVIES_DIR / "Movie Name" / "Movie Name.strm"
        result = convert_to_emby_path(path)
        assert result == str(path)
    
    def test_convert_to_emby_path_movies_mapping(self):
        """Test path conversion with movies path mapping"""
        os.environ["EMBY_MOVIES_PATH"] = "/mnt/media/movies"
        
        # Reload module
        if 'parse_m3u' in sys.modules:
            import importlib
            importlib.reload(sys.modules['parse_m3u'])
        
        from parse_m3u import convert_to_emby_path, MOVIES_DIR
        
        path = MOVIES_DIR / "Movie Name" / "Movie Name.strm"
        result = convert_to_emby_path(path)
        assert result == "/mnt/media/movies/Movie Name/Movie Name.strm"
    
    def test_convert_to_emby_path_series_mapping(self):
        """Test path conversion with series path mapping"""
        os.environ["EMBY_SERIES_PATH"] = "/mnt/media/series"
        
        # Reload module
        if 'parse_m3u' in sys.modules:
            import importlib
            importlib.reload(sys.modules['parse_m3u'])
        
        from parse_m3u import convert_to_emby_path, SERIES_DIR
        
        path = SERIES_DIR / "Series Name" / "Season 1" / "S01E01.strm"
        result = convert_to_emby_path(path)
        assert result == "/mnt/media/series/Series Name/Season 1/S01E01.strm"


class TestEmbyIntegration:
    """Test Emby API integration functions"""
    
    def setup_method(self):
        """Set up test environment"""
        self.original_url = os.environ.get("EMBY_SERVER_URL")
        self.original_key = os.environ.get("EMBY_API_KEY")
        os.environ["EMBY_SERVER_URL"] = "http://emby:8096"
        os.environ["EMBY_API_KEY"] = "test-api-key"
        
        # Reload module
        if 'parse_m3u' in sys.modules:
            import importlib
            importlib.reload(sys.modules['parse_m3u'])
    
    def teardown_method(self):
        """Restore environment"""
        if self.original_url:
            os.environ["EMBY_SERVER_URL"] = self.original_url
        elif "EMBY_SERVER_URL" in os.environ:
            del os.environ["EMBY_SERVER_URL"]
        
        if self.original_key:
            os.environ["EMBY_API_KEY"] = self.original_key
        elif "EMBY_API_KEY" in os.environ:
            del os.environ["EMBY_API_KEY"]
        
        # Reload module
        if 'parse_m3u' in sys.modules:
            import importlib
            importlib.reload(sys.modules['parse_m3u'])
    
    def test_get_emby_item_by_path_success(self, requests_mock):
        """Test successful item lookup by path"""
        from parse_m3u import get_emby_item_by_path
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        
        item_id = get_emby_item_by_path(Path("/test/path"))
        assert item_id == "12345"
    
    def test_get_emby_item_by_path_not_found(self, requests_mock):
        """Test item lookup when item not found"""
        from parse_m3u import get_emby_item_by_path
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path&Recursive=false",
            json={"Items": []},
            status_code=200
        )
        
        item_id = get_emby_item_by_path(Path("/test/path"))
        assert item_id is None
    
    def test_get_emby_item_by_path_error(self, requests_mock):
        """Test item lookup with API error"""
        from parse_m3u import get_emby_item_by_path
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path&Recursive=false",
            status_code=500
        )
        
        item_id = get_emby_item_by_path(Path("/test/path"))
        assert item_id is None
    
    def test_refresh_emby_item_success(self, requests_mock):
        """Test successful item refresh"""
        from parse_m3u import refresh_emby_item
        
        requests_mock.post(
            "http://emby:8096/emby/Items/12345/Refresh?Recursive=false&ImageRefreshMode=FullRefresh&MetadataRefreshMode=FullRefresh&ReplaceAllImages=false&ReplaceAllMetadata=false",
            status_code=204
        )
        
        result = refresh_emby_item("12345", "Update")
        assert result is True
    
    def test_refresh_emby_library_path_success(self, requests_mock):
        """Test successful library path refresh"""
        from parse_m3u import refresh_emby_library_path
        
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test/path&Recursive=true",
            status_code=204
        )
        
        result = refresh_emby_library_path(Path("/test/path"))
        assert result is True
    
    def test_notify_emby_new_file(self, requests_mock):
        """Test Emby notification for new file"""
        from parse_m3u import notify_emby
        
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test&Recursive=true",
            status_code=204
        )
        
        notify_emby(Path("/test/file.strm"), is_new=True, url_changed=False)
        
        assert requests_mock.call_count == 1
    
    def test_notify_emby_updated_file(self, requests_mock):
        """Test Emby notification for updated file"""
        from parse_m3u import notify_emby
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/file.strm&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        requests_mock.post(
            "http://emby:8096/emby/Items/12345/Refresh?Recursive=false&ImageRefreshMode=FullRefresh&MetadataRefreshMode=FullRefresh&ReplaceAllImages=false&ReplaceAllMetadata=false",
            status_code=204
        )
        
        notify_emby(Path("/test/file.strm"), is_new=False, url_changed=True)
        
        assert requests_mock.call_count == 2
    
    def test_notify_emby_no_changes(self, requests_mock):
        """Test Emby notification skipped when no changes"""
        from parse_m3u import notify_emby
        
        notify_emby(Path("/test/file.strm"), is_new=False, url_changed=False)
        
        assert requests_mock.call_count == 0


class TestPlaylistProcessing:
    """Test M3U playlist processing"""
    
    def setup_method(self):
        """Set up temporary directory"""
        self.temp_dir = Path(tempfile.mkdtemp())
        self.movies_dir = self.temp_dir / "movies"
        self.series_dir = self.temp_dir / "series"
        self.livetv_dir = self.temp_dir / "livetv"
        self.tmp_playlist = self.temp_dir / "playlist.m3u"
        
        # Patch the directory paths and temporary playlist
        self.movies_patcher = patch('parse_m3u.MOVIES_DIR', self.movies_dir)
        self.series_patcher = patch('parse_m3u.SERIES_DIR', self.series_dir)
        self.livetv_patcher = patch('parse_m3u.LIVETV_DIR', self.livetv_dir)
        self.tmp_playlist_patcher = patch('parse_m3u.TMP_PLAYLIST', self.tmp_playlist)
        self.movies_patcher.start()
        self.series_patcher.start()
        self.livetv_patcher.start()
        self.tmp_playlist_patcher.start()
    
    def teardown_method(self):
        """Clean up"""
        self.movies_patcher.stop()
        self.series_patcher.stop()
        self.livetv_patcher.stop()
        self.tmp_playlist_patcher.stop()
        if self.temp_dir.exists():
            shutil.rmtree(self.temp_dir)
    
    def test_process_playlist_movies(self):
        """Test processing movies from playlist"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="EN - Test Movie (2023)" tvg-id="" tvg-logo="" group-title="Movies",Test Movie (2023)
http://example.com/movie/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby'):
            process_playlist()
        
        # Check movie file was created
        movie_file = self.movies_dir / "Test Movie (2023)" / "Test Movie (2023).strm"
        assert movie_file.exists()
        assert movie_file.read_text().strip() == "http://example.com/movie/12345"
    
    def test_process_playlist_series(self):
        """Test processing series from playlist"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="EN - Test Series (2023) S01E01" tvg-id="" tvg-logo="" group-title="Series",Test Series (2023) S01E01
http://example.com/series/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby'):
            process_playlist()
        
        # Check series file was created
        series_file = self.series_dir / "Test Series (2023)" / "Season 1" / "S01E01.strm"
        assert series_file.exists()
        assert series_file.read_text().strip() == "http://example.com/series/12345"
    
    def test_process_playlist_live_tv(self):
        """Test processing live TV from playlist"""
        from parse_m3u import process_playlist
        
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Live Channel" tvg-id="" tvg-logo="" group-title="Live TV",Live Channel
http://example.com/live/12345
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby'):
            process_playlist()
        
        # Check live TV file was created
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        assert "Live Channel" in content
        assert "http://example.com/live/12345" in content


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

