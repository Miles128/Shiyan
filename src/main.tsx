import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import App from "./App";
import ErrorBoundary from "./ErrorBoundary";
import { AppConfigProvider, VocabProvider } from "./store";
import { ToastProvider } from "./components/Toaster";
import Home from "./pages/Home";
import Reader from "./pages/Reader";
import Vocab from "./pages/Vocab";
import Settings from "./pages/Settings";
import Stats from "./pages/Stats";
import Placement from "./pages/Placement";
import Books from "./pages/Books";
import DailyStudy from "./pages/DailyStudy";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary>
      <ToastProvider>
        <AppConfigProvider>
        <VocabProvider>
          <HashRouter>
            <Routes>
              <Route path="/" element={<App />}>
                <Route index element={<Home />} />
                <Route path="article/:id" element={<Reader />} />
                <Route path="vocab" element={<Vocab />} />
                <Route path="books" element={<Books />} />
                <Route path="daily" element={<DailyStudy />} />
                <Route path="stats" element={<Stats />} />
                <Route path="settings" element={<Settings />} />
                <Route path="placement" element={<Placement />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Route>
            </Routes>
          </HashRouter>
        </VocabProvider>
        </AppConfigProvider>
      </ToastProvider>
    </ErrorBoundary>
  </React.StrictMode>,
);
