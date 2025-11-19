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
    
    def test_convert_to_emby_path_no_mapping(self):
        """Test path conversion when no mapping is set"""
        # Patch constants to None (no mapping)
        with patch('parse_m3u.EMBY_MOVIES_PATH', None), \
             patch('parse_m3u.EMBY_SERIES_PATH', None), \
             patch('parse_m3u.EMBY_LIVETV_PATH', None):
            
            from parse_m3u import convert_to_emby_path, MOVIES_DIR
            path = MOVIES_DIR / "Movie Name" / "Movie Name.strm"
            result = convert_to_emby_path(path)
            assert result == str(path)
    
    def test_convert_to_emby_path_movies_mapping(self):
        """Test path conversion with movies path mapping"""
        # Patch constant to use mapping
        with patch('parse_m3u.EMBY_MOVIES_PATH', "/mnt/media/movies"):
            from parse_m3u import convert_to_emby_path, MOVIES_DIR
            path = MOVIES_DIR / "Movie Name" / "Movie Name.strm"
            result = convert_to_emby_path(path)
            assert result == "/mnt/media/movies/Movie Name/Movie Name.strm"
    
    def test_convert_to_emby_path_series_mapping(self):
        """Test path conversion with series path mapping"""
        # Patch constant to use mapping
        with patch('parse_m3u.EMBY_SERIES_PATH', "/mnt/media/series"):
            from parse_m3u import convert_to_emby_path, SERIES_DIR
            path = SERIES_DIR / "Series Name" / "Season 1" / "S01E01.strm"
            result = convert_to_emby_path(path)
            assert result == "/mnt/media/series/Series Name/Season 1/S01E01.strm"


class TestEmbyIntegration:
    """Test Emby API integration functions"""
    
    def setup_method(self):
        """Set up test environment"""
        # Patch constants directly instead of using environment variables
        self.emby_url_patcher = patch('parse_m3u.EMBY_SERVER_URL', "http://emby:8096")
        self.emby_key_patcher = patch('parse_m3u.EMBY_API_KEY', "test-api-key")
        self.emby_url_patcher.start()
        self.emby_key_patcher.start()
    
    def teardown_method(self):
        """Restore environment"""
        self.emby_url_patcher.stop()
        self.emby_key_patcher.stop()
    
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
    
    def test_notify_emby_updated_file(self, requests_mock):
        """Test Emby notification for updated file"""
        from parse_m3u import notify_emby_updated
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/file.strm&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        requests_mock.post(
            "http://emby:8096/emby/Items/12345/Refresh?Recursive=false&ImageRefreshMode=FullRefresh&MetadataRefreshMode=FullRefresh&ReplaceAllImages=false&ReplaceAllMetadata=false",
            status_code=204
        )
        
        notify_emby_updated(Path("/test/file.strm"))
        
        assert requests_mock.call_count == 2
    
    def test_notify_emby_updated_file_not_found(self, requests_mock):
        """Test Emby notification for updated file when item not found"""
        from parse_m3u import notify_emby_updated
        
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/file.strm&Recursive=false",
            json={"Items": []},
            status_code=200
        )
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test&Recursive=true",
            status_code=204
        )
        
        notify_emby_updated(Path("/test/file.strm"))
        
        assert requests_mock.call_count == 2
    
    def test_batch_refresh_directories(self, requests_mock):
        """Test batch refresh of directories with new files"""
        from parse_m3u import batch_refresh_directories
        
        directories = {Path("/test/movies/movie1"), Path("/test/movies/movie2")}
        
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test/movies/movie1&Recursive=true",
            status_code=204
        )
        requests_mock.post(
            "http://emby:8096/emby/Library/Refresh?Path=/test/movies/movie2&Recursive=true",
            status_code=204
        )
        
        batch_refresh_directories(directories)
        
        assert requests_mock.call_count == 2
    
    def test_delete_emby_item_success(self, requests_mock):
        """Test successful item deletion from Emby"""
        from parse_m3u import delete_emby_item
        
        requests_mock.delete(
            "http://emby:8096/emby/Items/12345",
            status_code=204
        )
        
        result = delete_emby_item("12345")
        assert result is True
    
    def test_delete_emby_item_error(self, requests_mock):
        """Test item deletion error handling"""
        from parse_m3u import delete_emby_item
        
        requests_mock.delete(
            "http://emby:8096/emby/Items/12345",
            status_code=404
        )
        
        result = delete_emby_item("12345")
        assert result is False


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
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
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
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
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
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Check live TV file was created
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        assert "Live Channel" in content
        assert "http://example.com/live/12345" in content
    
    def test_process_playlist_live_tv_url_pattern(self):
        """Test that URLs without /movie/ or /series/ go to live TV even with Movies group-title"""
        from parse_m3u import process_playlist
        
        # This simulates a live TV channel with URL like: http://example.com/username/password/channel_id
        playlist_content = """#EXTM3U
#EXTINF:-1 tvg-name="Movie Channel" tvg-id="" tvg-logo="" group-title="Movies",Movie Channel
http://example.com/username/password/1917227
"""
        self.tmp_playlist.write_text(playlist_content)
        
        with patch('parse_m3u.notify_emby_updated'), patch('parse_m3u.batch_refresh_directories'):
            process_playlist()
        
        # Should NOT create a movie STRM file
        movie_files = list(self.movies_dir.rglob("*.strm"))
        assert len(movie_files) == 0, "Live TV URL should not create movie STRM files"
        
        # Should create live TV entry
        live_file = self.livetv_dir / "livetv.m3u"
        assert live_file.exists()
        content = live_file.read_text()
        assert "Movie Channel" in content
        assert "1917227" in content
    
    def test_cleanup_orphaned_files_with_emby(self, requests_mock):
        """Test cleanup of orphaned files with Emby deletion"""
        from parse_m3u import cleanup_orphaned_files, REMOVE_ORPHANED
        
        # Create an orphaned file
        orphaned_file = self.movies_dir / "Orphaned Movie" / "Orphaned Movie.strm"
        orphaned_file.parent.mkdir(parents=True, exist_ok=True)
        orphaned_file.write_text("http://example.com/orphaned")
        
        # Mock Emby API calls
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path/Orphaned%20Movie/Orphaned%20Movie.strm&Recursive=false",
            json={"Items": [{"Id": "12345"}]},
            status_code=200
        )
        requests_mock.delete(
            "http://emby:8096/emby/Items/12345",
            status_code=204
        )
        
        # Patch REMOVE_ORPHANED and Emby config
        with patch('parse_m3u.REMOVE_ORPHANED', True), \
             patch('parse_m3u.EMBY_SERVER_URL', 'http://emby:8096'), \
             patch('parse_m3u.EMBY_API_KEY', 'test-key'), \
             patch('parse_m3u.convert_to_emby_path', return_value='/test/path/Orphaned Movie/Orphaned Movie.strm'):
            processed_files = set()  # No processed files, so orphaned_file is orphaned
            cleanup_orphaned_files(processed_files, self.movies_dir, "movie")
        
        # Verify file was deleted
        assert not orphaned_file.exists()
        # Verify Emby deletion was called
        assert requests_mock.call_count == 2  # GET to find item, DELETE to remove it
    
    def test_cleanup_orphaned_files_no_emby_item(self, requests_mock):
        """Test cleanup of orphaned files when item not found in Emby"""
        from parse_m3u import cleanup_orphaned_files
        
        # Create an orphaned file
        orphaned_file = self.movies_dir / "Orphaned Movie" / "Orphaned Movie.strm"
        orphaned_file.parent.mkdir(parents=True, exist_ok=True)
        orphaned_file.write_text("http://example.com/orphaned")
        
        # Mock Emby API call - item not found
        requests_mock.get(
            "http://emby:8096/emby/Items?Path=/test/path/Orphaned%20Movie/Orphaned%20Movie.strm&Recursive=false",
            json={"Items": []},
            status_code=200
        )
        
        # Patch REMOVE_ORPHANED and Emby config
        with patch('parse_m3u.REMOVE_ORPHANED', True), \
             patch('parse_m3u.EMBY_SERVER_URL', 'http://emby:8096'), \
             patch('parse_m3u.EMBY_API_KEY', 'test-key'), \
             patch('parse_m3u.convert_to_emby_path', return_value='/test/path/Orphaned Movie/Orphaned Movie.strm'):
            processed_files = set()
            cleanup_orphaned_files(processed_files, self.movies_dir, "movie")
        
        # Verify file was still deleted even if not in Emby
        assert not orphaned_file.exists()
        # Verify only GET was called (to find item), no DELETE
        assert requests_mock.call_count == 1


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

